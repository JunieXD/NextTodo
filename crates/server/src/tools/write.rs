//! Tools that change data. Each one builds its operations from the data inside the transaction that
//! applies them, so paths and numbers resolve against exactly what the change applies to.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use nexttodo_core::board::{Board, Snapshot, Status};
use nexttodo_core::model::{Change, Due, Effect, FolderId, MutationId, Name, Task, TaskId};
use nexttodo_core::ops::{MAX_OPS, Op};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use super::args::{
    AddArgs, ApplyArgs, DeleteArgs, DepArgs, EditArgs, MkdirArgs, MoveArgs, Plan, PlanRef, PlanTask, RenameArgs, TasksArgs, UndoArgs,
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
            None => Ok(Output {
                text: format!("批次 {} 已保存，本次未重复生效", done.seq),
                data: json!({ "batch": done.seq, "effects": done.effects, "unlocked": [] }),
            }),
        };
    }
    let snap = store::snapshot(&tx, &auth.user_id)?;
    let write = build(&WriteCx { c: &tx, owner: &auth.user_id, board: &Board::new(&snap) })?;
    if write.ops.len() > MAX_OPS {
        return Err(ApiError::invalid(format!("每次最多 {MAX_OPS} 个操作，本次有 {} 个", write.ops.len()), None));
    }
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
    lines.extend(text::changes(effects, &mut Names::new(after), 20));
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
    match unlocked.as_slice() {
        [] => {}
        [t] => lines.push(format!("解锁 #{} {}", t.number, t.title.as_str())),
        list => lines.push(format!(
            "解锁 {} 项：{}",
            list.len(),
            list.iter().map(|t| format!("#{} {}", t.number, t.title.as_str())).collect::<Vec<_>>().join("、")
        )),
    }
    if let Some(batch) = batch {
        lines.push(format!("批次 {batch}，可撤销"));
    }
    let unlocked: Vec<Value> = unlocked.iter().map(|t| task_view(&b, t)).collect();
    Output { text: lines.join("\n"), data: json!({ "batch": batch, "effects": effects, "unlocked": unlocked }) }
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

fn due(due: Option<super::args::DueText>, remind: Option<u32>) -> Result<Option<Due>, ApiError> {
    match (due, remind) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(ApiError::invalid("设置提醒需要截止时间", Some("remind".into()))),
        (Some(d), remind) => Due::new(d.date, d.time, remind).map(Some).map_err(|m| ApiError::invalid(m, Some("due".into()))),
    }
}

/// A task number written as text in a plan: `"42"` or `"#42"`.
pub(crate) fn number(s: &str) -> Option<i64> {
    s.strip_prefix('#').unwrap_or(s).parse().ok()
}

/// The operations adding `tasks` to `folder` as those of a plan for it: what AI organizing adds.
pub(crate) fn plan_into(b: &Board, folder: Option<FolderId>, tasks: Vec<PlanTask>) -> Result<Vec<Op>, ApiError> {
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
        let PlanTask { name: _, title, description, folder, due: due_text, remind, after, before } = t;
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
            due: due(due_text, remind)?,
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
    let b = w.board;
    if a.title.is_none() && a.description.is_none() && a.due.is_none() && a.remind.is_none() {
        return Err(ApiError::invalid("未指定要修改的内容", None));
    }
    let t = find_task(b, a.task)?;
    let current_remind = t.due.as_ref().and_then(|d| d.at.as_ref()).and_then(|at| at.remind_before);
    let new_due = match (a.due, a.remind) {
        (None, None) => None,
        (Some(None), Some(Some(_))) => return Err(ApiError::invalid("清除截止时间时不能同时设置提醒", Some("remind".into()))),
        (Some(None), _) => Some(None),
        (Some(Some(d)), remind) => {
            // Moving the deadline keeps the reminder as long as there is still a time to remind before.
            let remind = remind.unwrap_or(if d.time.is_some() { current_remind } else { None });
            Some(due(Some(d), remind)?)
        }
        (None, Some(remind)) => match t.due.as_ref().and_then(|d| d.at.as_ref().map(|at| (d.date.clone(), at.time.clone()))) {
            Some((date, time)) => Some(Some(Due::new(date, Some(time), remind).map_err(|m| ApiError::invalid(m, Some("remind".into())))?)),
            // Without a time there is no reminder, so clearing it changes nothing.
            None if remind.is_none() => None,
            None => return Err(ApiError::invalid("该事项没有具体的截止时间，无法设置提醒", Some("remind".into()))),
        },
    };
    Ok(vec![Op::TaskUpdate {
        id: t.id,
        folder_id: None,
        title: a.title,
        description: a.description,
        due: new_due,
        sort_key: None,
        done: None,
    }]
    .into())
}

fn set_done(b: &Board, tasks: Vec<i64>, done: bool) -> Result<Write, ApiError> {
    if tasks.is_empty() {
        return Err(ApiError::invalid("至少需要指定一个事项", Some("tasks".into())));
    }
    let mut seen = HashSet::new();
    let mut ops = Vec::new();
    for n in tasks {
        let id = find_task(b, n)?.id;
        if seen.insert(id) {
            ops.push(Op::TaskUpdate { id, folder_id: None, title: None, description: None, due: None, sort_key: None, done: Some(done) });
        }
    }
    Ok(ops.into())
}

pub fn done(w: &WriteCx, a: TasksArgs) -> Result<Write, ApiError> {
    let b = w.board;
    set_done(b, a.tasks, true)
}

pub fn reopen(w: &WriteCx, a: TasksArgs) -> Result<Write, ApiError> {
    let b = w.board;
    set_done(b, a.tasks, false)
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
        ops.push(Op::TaskUpdate { id: t.id, folder_id: Some(to), title: None, description: None, due: None, sort_key: None, done: None });
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
