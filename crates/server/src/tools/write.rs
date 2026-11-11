//! Tools that change data. Each one builds its operations from the data inside the transaction that
//! applies them, so paths and numbers resolve against exactly what the change applies to.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use nexttodo_core::board::{Board, Snapshot, Status};
use nexttodo_core::model::{Change, Date, Description, Due, Effect, FolderId, MutationId, Name, Repeat, RepeatFrom, Task, TaskId, Title};
use nexttodo_core::ops::Op;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use uuid::Uuid;

use super::args::{
    AddArgs, ApplyArgs, DeleteArgs, DepArgs, DoneArgs, EditArgs, MkdirArgs, MoveArgs, Plan, PlanRef, PlanTask, RenameArgs, RepeatText,
    Replace, TasksArgs, UndoArgs,
};
use super::text::{self, Names, task_view};
use super::{Output, WriteFn, find_folder, find_task};
use crate::auth::Auth;
use crate::error::ApiError;
use crate::mutate::{self, Actor};
use crate::store;

/// What write tools build from.
pub struct WriteCx<'a> {
    pub c: &'a Connection,
    pub owner: &'a str,
    pub board: &'a Board<'a>,
}

/// The operations a write tool wants applied.
pub struct Write {
    pub ops: Vec<Op>,
    /// Show what would change without saving it.
    pub preview: bool,
}

impl From<Vec<Op>> for Write {
    fn from(ops: Vec<Op>) -> Self {
        Write { ops, preview: false }
    }
}

pub fn run(c: &mut Connection, auth: &Auth, id: Option<MutationId>, now: i64, build: WriteFn) -> Result<Output, ApiError> {
    let actor = Actor { owner: &auth.user_id, token_id: &auth.token_id, via: auth.via() };
    let id = id.unwrap_or_else(|| MutationId(Uuid::now_v7()));
    let tx = c.transaction()?;
    if let Some(done) = mutate::earlier(&tx, &actor, id)? {
        // A retry gets the first answer again, not one made from what has changed since.
        let reply: Option<String> = tx.query_row("SELECT reply FROM mutations WHERE seq = ?1", [done.seq], |r| r.get(0))?;
        return match reply {
            Some(reply) => Ok(serde_json::from_str(&reply)?),
            // Past the retention period, or sent by an app rather than through a tool. Only the
            // receipt is given: the old effects cannot be read against today's folders.
            None => {
                Ok(Output { text: format!("批次 {} 已保存，本次未重复生效", done.seq), data: json!({ "batch": done.seq }) })
            }
        };
    }
    let snap = store::snapshot(&tx, &auth.user_id)?;
    let write = build(&WriteCx { c: &tx, owner: &auth.user_id, board: &Board::new(&snap) })?;
    let effects = mutate::run(&tx, &actor, &write.ops, now)?;
    // Rendered inside the transaction, so a preview still sees what it would have made.
    let after = store::snapshot(&tx, &auth.user_id)?;
    if write.preview {
        return Ok(report(&after, None, &effects));
    }
    let batch = mutate::record(&tx, &actor, id, &write.ops, &effects, now)?;
    let output = report(&after, Some(batch), &effects);
    tx.execute("UPDATE mutations SET reply = ?1 WHERE seq = ?2", params![serde_json::to_string(&output)?, batch])?;
    tx.commit()?;
    Ok(output)
}

fn report(after: &Snapshot, batch: Option<i64>, effects: &[Effect]) -> Output {
    let b = Board::new(after);
    let mut lines = Vec::new();
    if batch.is_none() {
        lines.push("预览（未保存）：".to_owned());
    }
    let (said, changes) = text::changes(effects, &mut Names::new(after), 20);
    lines.extend(said);
    if effects.is_empty() {
        lines.push("没有变化".into());
    }
    let mut unlocked: Vec<&Task> = Vec::new();
    for e in effects {
        if let Effect::Task(Change::Updated { before, after }) = e
            && before.done_at.is_none()
            && after.done_at.is_some()
            && let Some(t) = b.task(after.id)
        {
            for a in b.after(t) {
                if b.status(a) == Status::Ready && !unlocked.iter().any(|u| u.id == a.id) {
                    unlocked.push(a);
                }
            }
        }
    }
    b.sort(&mut unlocked);
    let mut data = json!({ "batch": batch, "changes": changes });
    match unlocked.as_slice() {
        [] => {}
        [t] => lines.push(format!("解锁 {}", text::item(t))),
        list => lines.push(format!("解锁 {} 项：{}", list.len(), list.iter().map(|t| text::item(t)).collect::<Vec<_>>().join("、"))),
    }
    if !unlocked.is_empty() {
        data["unlocked"] = unlocked.iter().map(|t| task_view(&b, t)).collect();
    }
    if let Some(batch) = batch {
        lines.push(format!("批次 {batch}，可撤销"));
    }
    Output { text: lines.join("\n"), data }
}

/// Finds folders by path, adding operations to create the ones that are missing.
struct Folders<'a, 'b> {
    board: &'a Board<'b>,
    made: HashMap<String, FolderId>,
}

impl Folders<'_, '_> {
    /// The folder at `path` and its path written out in full.
    fn ensure(&mut self, ops: &mut Vec<Op>, path: &str) -> Result<(Option<FolderId>, String), ApiError> {
        let (mut cur, mut full) = (None, String::new());
        for name in path.split('/').map(str::trim).filter(|s| !s.is_empty()) {
            full = format!("{full}/{name}");
            let existing =
                self.made.get(&full).copied().or_else(|| self.board.subfolders(cur).iter().find(|f| f.name.as_str() == name).map(|f| f.id));
            cur = Some(match existing {
                Some(id) => id,
                None => {
                    let id = FolderId(Uuid::now_v7());
                    let name = Name::try_from(name.to_owned()).map_err(|m| ApiError::invalid(m, None))?;
                    ops.push(Op::FolderCreate { id, name, parent_id: cur, sort_key: None });
                    self.made.insert(full.clone(), id);
                    id
                }
            });
        }
        Ok((cur, if full.is_empty() { "/".into() } else { full }))
    }
}

fn due(due: Option<super::args::DueText>, remind: Option<u32>, repeat: Option<Repeat>) -> Result<Option<Due>, ApiError> {
    match (due, remind, repeat) {
        (None, None, None) => Ok(None),
        (None, Some(_), _) => Err(ApiError::invalid("设置提醒需要截止时间", Some("remind".into()))),
        (None, None, Some(_)) => Err(ApiError::invalid("设置重复需要截止时间", Some("repeat".into()))),
        (Some(d), remind, repeat) => {
            Due::new(d.date, d.time, remind, repeat).map(Some).map_err(|m| ApiError::invalid(m, Some("due".into())))
        }
    }
}

/// A new task's due date, as a plan gives it.
pub(crate) fn new_due(
    due_text: Option<super::args::DueText>,
    remind: Option<u32>,
    repeat_text: Option<RepeatText>,
    from: Option<RepeatFrom>,
) -> Result<Option<Due>, ApiError> {
    due(due_text, remind, repeat(repeat_text, from)?)
}

/// The repeat given, from the due date unless said.
fn repeat(repeat: Option<RepeatText>, from: Option<RepeatFrom>) -> Result<Option<Repeat>, ApiError> {
    match (repeat, from) {
        (None, Some(_)) => Err(ApiError::invalid("repeat_from 只能和 repeat 一起给", Some("repeat_from".into()))),
        (r, from) => Ok(r.map(|r| Repeat { every: r.every, period: r.period, from: from.unwrap_or(RepeatFrom::Due) })),
    }
}

/// A task number written as text in a plan: `"42"` or `"#42"`.
pub(crate) fn number(s: &str) -> Option<i64> {
    s.strip_prefix('#').unwrap_or(s).parse().ok()
}

/// The operations adding `tasks` to `folder` as those of a plan for it: what AI organizing adds.
pub fn plan_into(b: &Board, folder: Option<FolderId>, tasks: Vec<PlanTask>) -> Result<Vec<Op>, ApiError> {
    plan_ops(b, Plan { folder: Some(b.path(folder)), tasks })
}

fn plan_ops(b: &Board, plan: Plan) -> Result<Vec<Op>, ApiError> {
    let mut ops = Vec::new();
    let mut folders = Folders { board: b, made: HashMap::new() };
    let (base, base_path) = folders.ensure(&mut ops, plan.folder.as_deref().unwrap_or("/"))?;
    let ids: Vec<TaskId> = plan.tasks.iter().map(|_| TaskId(Uuid::now_v7())).collect();
    let mut refs = HashMap::new();
    for (t, &id) in plan.tasks.iter().zip(&ids) {
        let Some(name) = &t.name else { continue };
        if number(name).is_some() {
            return Err(ApiError::invalid(format!("ref“{name}”会被识别为事项编号，请改用非数字的名称"), None));
        }
        if refs.insert(name.clone(), id).is_some() {
            return Err(ApiError::invalid(format!("计划中有多个事项的 ref 均为“{name}”"), None));
        }
    }
    let resolve = |r: &PlanRef| -> Result<TaskId, ApiError> {
        match r {
            PlanRef::Number(n) => Ok(find_task(b, *n)?.id),
            PlanRef::Name(s) => match number(s) {
                Some(n) => Ok(find_task(b, n)?.id),
                None => refs.get(s).copied().ok_or_else(|| ApiError::invalid(format!("计划中没有 ref 为“{s}”的事项"), None)),
            },
        }
    };
    let mut links = Vec::new();
    for (t, &id) in plan.tasks.into_iter().zip(&ids) {
        let PlanTask { name: _, title, description, folder, due: due_text, remind, repeat: repeat_text, repeat_from, after, before } = t;
        let folder_id = match folder {
            None => base,
            Some(p) if p.trim_start().starts_with('/') => folders.ensure(&mut ops, &p)?.0,
            Some(p) => folders.ensure(&mut ops, &format!("{base_path}/{p}"))?.0,
        };
        ops.push(Op::TaskCreate {
            id,
            title,
            folder_id,
            description: description.unwrap_or_default(),
            due: new_due(due_text, remind, repeat_text, repeat_from)?,
            sort_key: None,
            done: false,
        });
        for r in &after {
            links.push((resolve(r)?, id));
        }
        for r in &before {
            links.push((id, resolve(r)?));
        }
    }
    let mut seen = HashSet::new();
    ops.extend(links.into_iter().filter(|l| seen.insert(*l)).map(|(before, after)| Op::DepAdd { before, after }));
    Ok(ops)
}

pub fn add(w: &WriteCx, a: AddArgs) -> Result<Write, ApiError> {
    let b = w.board;
    let task = PlanTask {
        name: None,
        title: a.title,
        description: a.description,
        folder: None,
        due: a.due,
        remind: a.remind,
        repeat: a.repeat,
        repeat_from: a.repeat_from,
        after: a.after.into_iter().map(PlanRef::Number).collect(),
        before: a.before.into_iter().map(PlanRef::Number).collect(),
    };
    plan_ops(b, Plan { folder: a.folder, tasks: vec![task] }).map(Write::from)
}

pub fn apply(w: &WriteCx, a: ApplyArgs) -> Result<Write, ApiError> {
    let b = w.board;
    if a.plan.tasks.is_empty() {
        return Err(ApiError::invalid("计划至少包含一个事项", Some("plan.tasks".into())));
    }
    Ok(Write { ops: plan_ops(b, a.plan)?, preview: a.dry_run })
}

pub fn edit(w: &WriteCx, a: EditArgs) -> Result<Write, ApiError> {
    let Edit { id, title, description, due } = edit_fields(w.board, a)?;
    Ok(vec![Op::TaskUpdate { id, folder_id: None, title, description, due, sort_key: None, done: None, today: None }].into())
}

/// The fields `edit` sets on a task, the due date in full; `None` where it leaves one as it is.
pub(crate) struct Edit {
    pub id: TaskId,
    pub title: Option<Title>,
    pub description: Option<Description>,
    pub due: Option<Option<Due>>,
}

/// What `edit` changes: also what AI tidying suggests changing in a task.
pub(crate) fn edit_fields(b: &Board, a: EditArgs) -> Result<Edit, ApiError> {
    let revising = !a.replace.is_empty() || a.append.is_some();
    let changes_repeat = a.repeat.is_some() || a.repeat_from.is_some();
    if a.title.is_none() && a.description.is_none() && !revising && a.due.is_none() && a.remind.is_none() && !changes_repeat {
        return Err(ApiError::invalid("未指定要修改的内容", None));
    }
    if a.description.is_some() && revising {
        return Err(ApiError::invalid("description 是整段替换，不能和 replace、append 同时给", Some("description".into())));
    }
    let t = find_task(b, a.task)?;
    let description = if revising { Some(revise(t.description.as_str(), a.replace, a.append)?) } else { a.description };
    let current = t.due.as_ref();
    let current_remind = current.and_then(|d| d.at.as_ref()).and_then(|at| at.remind_before);
    // `Some(None)` stops it repeating.
    let new_repeat = if changes_repeat { Some(repeat(a.repeat.flatten(), a.repeat_from)?) } else { None };
    let no_time = || ApiError::invalid("该事项没有具体的截止时间，无法设置提醒", Some("remind".into()));
    let new_due = match (a.due, a.remind) {
        (None, None) if new_repeat.is_none() => None,
        (Some(None), remind) => {
            if matches!(remind, Some(Some(_))) {
                return Err(ApiError::invalid("清除截止时间时不能同时设置提醒", Some("remind".into())));
            }
            if matches!(new_repeat, Some(Some(_))) {
                return Err(ApiError::invalid("清除截止时间时不能同时设置重复", Some("repeat".into())));
            }
            Some(None)
        }
        (Some(Some(d)), remind) => {
            // Moving the deadline keeps the reminder as long as there is still a time to remind before,
            // and the repeat.
            let remind = remind.unwrap_or(if d.time.is_some() { current_remind } else { None });
            Some(due(Some(d), remind, new_repeat.unwrap_or(current.and_then(|c| c.repeat)))?)
        }
        (None, remind) => match current {
            Some(c) => {
                if matches!(remind, Some(Some(_))) && c.at.is_none() {
                    return Err(no_time());
                }
                let time = c.at.as_ref().map(|at| at.time.clone());
                let new = Due::new(c.date.clone(), time, remind.unwrap_or(current_remind), new_repeat.unwrap_or(c.repeat))
                    .map_err(|m| ApiError::invalid(m, Some("remind".into())))?;
                // Clearing a reminder there isn't changes nothing.
                (Some(&new) != current).then_some(Some(new))
            }
            None if matches!(remind, Some(Some(_))) => return Err(no_time()),
            None if matches!(new_repeat, Some(Some(_))) => {
                return Err(ApiError::invalid("该事项没有截止时间，无法设置重复", Some("repeat".into())));
            }
            // Without a due date there is no reminder or repeat, so clearing them changes nothing.
            None => None,
        },
    };
    Ok(Edit { id: t.id, title: a.title, description, due: new_due })
}

/// The description as it is now with each `old` replaced by its `new` in turn, then `append` on a
/// line of its own at the end: only the part an agent changes, so that what someone else changed
/// meanwhile elsewhere in it stays.
fn revise(current: &str, replace: Vec<Replace>, append: Option<String>) -> Result<Description, ApiError> {
    let mut revised = current.to_owned();
    for Replace { old, new } in replace {
        let Some(first) = old.chars().next() else {
            return Err(ApiError::invalid("replace 的 old 不能为空", Some("replace".into())));
        };
        let Some(at) = revised.find(&old) else {
            return Err(ApiError::conflict(format!("描述里没有“{}”，可能已被改过，请先 show 再改", text::excerpt(&old, 30))));
        };
        // Also where it overlaps itself, as "aa" twice in "aaa".
        if revised[at + first.len_utf8()..].contains(&old) {
            return Err(ApiError::invalid(
                format!("描述里不止一处“{}”，请多带一些上下文，使它只出现一次", text::excerpt(&old, 30)),
                Some("replace".into()),
            ));
        }
        revised.replace_range(at..at + old.len(), &new);
    }
    if let Some(more) = append {
        if more.trim().is_empty() {
            return Err(ApiError::invalid("append 不能为空", Some("append".into())));
        }
        if !revised.is_empty() && !revised.ends_with('\n') {
            revised.push('\n');
        }
        revised.push_str(&more);
    }
    Description::try_from(revised).map_err(|m| ApiError::invalid(m, Some("description".into())))
}

/// Marks `tasks` done or open again; a repeating task done is due next time, counted from `today`,
/// unless `end_repeat`, which stops it repeating so that it is done.
fn set_done(b: &Board, tasks: Vec<i64>, done: bool, today: Option<Date>, end_repeat: bool) -> Result<Write, ApiError> {
    if tasks.is_empty() {
        return Err(ApiError::invalid("至少需要指定一个事项", Some("tasks".into())));
    }
    let mut seen = HashSet::new();
    let mut ops = Vec::new();
    for n in tasks {
        let t = find_task(b, n)?;
        if seen.insert(t.id) {
            let due = t.due.as_ref().filter(|d| end_repeat && d.repeat.is_some()).map(|d| Some(Due { repeat: None, ..d.clone() }));
            ops.push(Op::TaskUpdate {
                id: t.id,
                folder_id: None,
                title: None,
                description: None,
                due,
                sort_key: None,
                done: Some(done),
                today: today.clone(),
            });
        }
    }
    Ok(ops.into())
}

pub fn done(w: &WriteCx, a: DoneArgs) -> Result<Write, ApiError> {
    let b = w.board;
    set_done(b, a.tasks, true, a.today, a.end_repeat)
}

pub fn reopen(w: &WriteCx, a: TasksArgs) -> Result<Write, ApiError> {
    let b = w.board;
    set_done(b, a.tasks, false, None, false)
}

pub fn mkdir(w: &WriteCx, a: MkdirArgs) -> Result<Write, ApiError> {
    let b = w.board;
    let mut ops = Vec::new();
    let (_, path) = Folders { board: b, made: HashMap::new() }.ensure(&mut ops, &a.path)?;
    if ops.is_empty() {
        return Err(ApiError::conflict(format!("文件夹 {path} 已存在")));
    }
    Ok(ops.into())
}

/// A folder other than the root.
fn folder_id(b: &Board, path: &str, what: &str) -> Result<FolderId, ApiError> {
    find_folder(b, path)?.ok_or_else(|| ApiError::invalid(format!("根目录不能{what}"), None))
}

pub fn rename(w: &WriteCx, a: RenameArgs) -> Result<Write, ApiError> {
    let b = w.board;
    let id = folder_id(b, &a.folder, "改名")?;
    Ok(vec![Op::FolderUpdate { id, name: Some(a.name), parent_id: None, sort_key: None }].into())
}

pub fn move_to(w: &WriteCx, a: MoveArgs) -> Result<Write, ApiError> {
    let b = w.board;
    if a.tasks.is_empty() && a.folders.is_empty() {
        return Err(ApiError::invalid("未指定要移动的事项或文件夹", None));
    }
    let to = find_folder(b, &a.to)?;
    let mut ops = Vec::new();
    for n in a.tasks {
        let t = find_task(b, n)?;
        ops.push(Op::TaskUpdate {
            id: t.id,
            folder_id: Some(to),
            title: None,
            description: None,
            due: None,
            sort_key: None,
            done: None,
            today: None,
        });
    }
    for path in &a.folders {
        ops.push(Op::FolderUpdate { id: folder_id(b, path, "移动")?, name: None, parent_id: Some(to), sort_key: None });
    }
    Ok(ops.into())
}

pub fn delete(w: &WriteCx, a: DeleteArgs) -> Result<Write, ApiError> {
    let b = w.board;
    if a.tasks.is_empty() && a.folders.is_empty() {
        return Err(ApiError::invalid("未指定要删除的事项或文件夹", None));
    }
    let mut ops = Vec::new();
    let mut seen = HashSet::new();
    for n in a.tasks {
        let id = find_task(b, n)?.id;
        if seen.insert(id) {
            ops.push(Op::TaskDelete { id });
        }
    }
    let mut folders = Vec::new();
    for path in &a.folders {
        let id = folder_id(b, path, "删除")?;
        if !folders.contains(&id) {
            folders.push(id);
        }
    }
    // Deepest first, so that a folder inside another one listed here is still there when its turn comes.
    folders.sort_by_key(|&f| Reverse(std::iter::successors(Some(f), |id| b.folder(*id).and_then(|f| f.parent_id)).count()));
    ops.extend(folders.into_iter().map(|id| Op::FolderDelete { id }));
    Ok(ops.into())
}

pub fn dep_add(w: &WriteCx, a: DepArgs) -> Result<Write, ApiError> {
    let b = w.board;
    Ok(vec![Op::DepAdd { before: find_task(b, a.before)?.id, after: find_task(b, a.after)?.id }].into())
}

pub fn dep_remove(w: &WriteCx, a: DepArgs) -> Result<Write, ApiError> {
    let b = w.board;
    Ok(vec![Op::DepRemove { before: find_task(b, a.before)?.id, after: find_task(b, a.after)?.id }].into())
}

pub fn undo(w: &WriteCx, a: UndoArgs) -> Result<Write, ApiError> {
    let mutation: MutationId = w
        .c
        .query_row("SELECT id FROM mutations WHERE seq = ?1 AND owner_id = ?2 AND effects IS NOT NULL", params![a.batch, w.owner], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| ApiError::not_found(format!("批次 {} 不存在或已过期", a.batch)))?;
    Ok(vec![Op::Undo { mutation }].into())
}
