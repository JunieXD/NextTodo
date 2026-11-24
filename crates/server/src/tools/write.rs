//! Tools that change data. Each one builds its operations from the data inside the transaction that
//! applies them, so paths and numbers resolve against exactly what the change applies to.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use nexttodo_core::board::{Board, Now, Status};
use nexttodo_core::model::{
    Attachment, AttachmentId, Change, Date, Description, Due, DueAfter, Effect, FileName, FileSize, FolderId, MAX_NAME, MediaType, Misfit,
    MutationId, Name, Remind, Repeat, RepeatFrom, Task, TaskId, Time, Title,
};
use nexttodo_core::ops::{MAX_OPS, Op};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use uuid::Uuid;

use super::args::{
    AddArgs, ApplyArgs, AttachArgs, CopyArgs, DeleteArgs, DepArgs, DetachArgs, DoneArgs, DueAfterText, EditArgs, MkdirArgs, MoveArgs, Plan,
    PlanRef, PlanTask, RemindText, RenameArgs, RepeatText, Replace, ShareArgs, StyleArgs, TaskRef, TasksArgs, UndoArgs, UnshareArgs,
    WhenText, task_ref,
};
use super::text::{self, Names, task_view};
use super::{Output, WriteFn, find_folder, find_task, person};
use crate::attachments::copy_file;
use crate::auth::Auth;
use crate::error::ApiError;
use crate::files::Files;
use crate::mutate::{self, Actor};
use crate::store::{self, StoredFile};

/// What write tools build from.
pub struct WriteCx<'a> {
    pub c: &'a Connection,
    pub user: &'a str,
    pub board: &'a Board<'a>,
    /// As the user gave it: a repeating task done needs it.
    pub today: Option<Date>,
    pub now: i64,
    pub files: &'a Files,
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

/// Runs a write tool: its output, and the users its change went to.
pub fn run(
    c: &mut Connection,
    auth: &Auth,
    files: &Files,
    id: MutationId,
    now: i64,
    local: Option<Now>,
    build: &WriteFn,
) -> Result<(Output, Vec<String>), ApiError> {
    let actor = Actor { user: &auth.user_id, token_id: &auth.token_id, via: auth.via() };
    let tx = c.transaction()?;
    if let Some(output) = earlier(&tx, &actor, id)? {
        return Ok((output, Vec::new()));
    }
    let (ops, done) = match attempt(&tx, &actor, files, now, local.clone(), build)? {
        Attempt::Preview(output) => return Ok((output, Vec::new())),
        Attempt::Made { ops, done } => (ops, done),
    };
    let recorded = mutate::record(&tx, &actor, id, &ops, &done, now)?;
    let output = reported(&tx, &auth.user_id, now, local, recorded.seq, &done.view(&auth.user_id))?;
    tx.commit()?;
    Ok((output, recorded.to))
}

/// The answer to tool change `id` made before: a retry gets the first answer again, not one made
/// from what has changed since.
pub(crate) fn earlier(c: &Connection, actor: &Actor, id: MutationId) -> Result<Option<Output>, ApiError> {
    let Some(done) = mutate::earlier(c, actor, id)? else { return Ok(None) };
    let reply: Option<String> = c.query_row("SELECT reply FROM mutations WHERE seq = ?1", [done.seq], |r| r.get(0))?;
    Ok(Some(match reply {
        Some(reply) => serde_json::from_str(&reply)?,
        // Past the retention period, sent by an app rather than through a tool, or made through a
        // drive with this app stopped before it was written here. Only the receipt is given: the old
        // effects cannot be read against today's folders.
        None => Output { text: format!("批次 {} 已保存，本次未重复生效", done.seq), data: json!({ "batch": done.seq }) },
    }))
}

/// What a write tool does on `c`.
pub(crate) enum Attempt {
    /// It only shows what it would do.
    Preview(Output),
    Made {
        ops: Vec<Op>,
        done: mutate::Done,
    },
}

/// Builds the tool's operations from the data on `c` and applies them there, the caller to record or
/// roll them back.
pub(crate) fn attempt(
    c: &Connection,
    actor: &Actor,
    files: &Files,
    now: i64,
    local: Option<Now>,
    build: &WriteFn,
) -> Result<Attempt, ApiError> {
    let user = actor.user;
    let snap = store::snapshot(c, user)?;
    let today = local.as_ref().map(|l| l.today.clone());
    let board = Board::new(&snap, local.clone().unwrap_or_else(|| Now::utc(now)), user);
    let write = build(&WriteCx { c, user, board: &board, today, now, files })?;
    let done = mutate::run(c, actor, &write.ops, now, board.now())?;
    if write.preview {
        // Rendered before rolling back, so it still sees what it would have made.
        return Ok(Attempt::Preview(rendered(c, user, now, local, None, &done.view(user))?));
    }
    Ok(Attempt::Made { ops: write.ops, done })
}

/// The answer to a tool change recorded as `batch`, read against the data after it, and kept for a
/// retry.
pub(crate) fn reported(
    c: &Connection,
    user: &str,
    now: i64,
    local: Option<Now>,
    batch: i64,
    effects: &[Effect],
) -> Result<Output, ApiError> {
    let output = rendered(c, user, now, local, Some(batch), effects)?;
    c.execute("UPDATE mutations SET reply = ?1 WHERE seq = ?2", params![serde_json::to_string(&output)?, batch])?;
    Ok(output)
}

fn rendered(c: &Connection, user: &str, now: i64, local: Option<Now>, batch: Option<i64>, effects: &[Effect]) -> Result<Output, ApiError> {
    let after = store::snapshot(c, user)?;
    let b = Board::new(&after, local.unwrap_or_else(|| Now::utc(now)), user);
    let names = Names::new(&after, user, store::people(c, &text::strangers(&after, effects))?);
    Ok(report(&b, names, batch, effects))
}

/// What a change did, read with the `names` from after it.
fn report(b: &Board, mut names: Names, batch: Option<i64>, effects: &[Effect]) -> Output {
    let mut lines = Vec::new();
    if batch.is_none() {
        lines.push("预览（未保存）：".to_owned());
    }
    let (said, changes) = text::changes(effects, &mut names, 20);
    lines.extend(said);
    if effects.is_empty() {
        lines.push("没有变化".into());
    }
    let mut unlocked: Vec<&Task> = Vec::new();
    for e in effects {
        if let Effect::Task(Change::Updated { before, after }) = e
            && b.holds_up(before)
            && let Some(t) = b.task(after.id)
            && !b.holds_up(t)
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
        [t] => lines.push(format!("解锁 {}", text::item(b, t))),
        list => lines.push(format!("解锁 {} 项：{}", list.len(), list.iter().map(|t| text::item(b, t)).collect::<Vec<_>>().join("、"))),
    }
    if !unlocked.is_empty() {
        data["unlocked"] = unlocked.iter().map(|t| task_view(b, t)).collect();
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
    /// The folder at `path` and its path written out in full. In a folder someone else shares, made
    /// in there: that folder itself has to be there.
    fn ensure(&mut self, ops: &mut Vec<Op>, path: &str) -> Result<(Option<FolderId>, String), ApiError> {
        let (mut cur, mut full) = (None, String::new());
        let mut names = path.split('/').map(str::trim).filter(|s| !s.is_empty());
        if path.trim_start().starts_with('@') {
            full = format!("{}/{}", names.next().unwrap_or_default(), names.next().unwrap_or_default());
            cur = find_folder(self.board, &full)?;
        }
        for name in names {
            full = format!("{full}/{name}");
            let existing =
                self.made.get(&full).copied().or_else(|| self.board.subfolders(cur).iter().find(|f| f.name.as_str() == name).map(|f| f.id));
            cur = Some(match existing {
                Some(id) => id,
                None => {
                    let id = FolderId(Uuid::now_v7());
                    let name = Name::try_from(name.to_owned()).map_err(|m| ApiError::invalid(m, None))?;
                    ops.push(Op::FolderCreate { id, name, parent_id: cur, sort_key: None, color: None, icon: None });
                    self.made.insert(full.clone(), id);
                    id
                }
            });
        }
        Ok((cur, if full.is_empty() { "/".into() } else { full }))
    }
}

fn due(due: Option<WhenText>, remind: Vec<Remind>, repeat: Option<Repeat>) -> Result<Option<Due>, ApiError> {
    match (due, repeat) {
        (None, _) if !remind.is_empty() => Err(ApiError::invalid("设置提醒需要截止时间", Some("remind".into()))),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(ApiError::invalid("设置重复需要截止时间", Some("repeat".into()))),
        (Some(d), repeat) => Due::new(d.date, d.time, remind, repeat).map(Some).map_err(|m| ApiError::invalid(m, Some("due".into()))),
    }
}

/// A new task's due date as a plan gives it: a date, or some days after its prerequisites are done.
pub(crate) fn new_due(
    due_text: Option<WhenText>,
    after: Option<DueAfterText>,
    remind: Option<RemindText>,
    repeat_text: Option<RepeatText>,
    from: Option<RepeatFrom>,
    until: Option<Date>,
) -> Result<(Option<Due>, Option<DueAfter>), ApiError> {
    let repeat = repeat(repeat_text, from, until)?;
    let remind = remind.map_or_else(Vec::new, |r| r.0);
    match (due_text, after) {
        (due_text, None) => Ok((due(due_text, remind, repeat)?, None)),
        (Some(_), Some(_)) => Err(mutate::both_due()),
        (None, Some(_)) if repeat.is_some() => Err(repeating_after()),
        (None, Some(a)) => Ok((None, Some(due_after(a.days, a.time, remind)?))),
    }
}

fn due_after(days: u16, time: Option<Time>, remind: Vec<Remind>) -> Result<DueAfter, ApiError> {
    DueAfter::new(days, time, remind).map_err(|m| ApiError::invalid(m, Some("due_after".into())))
}

fn repeating_after() -> ApiError {
    ApiError::invalid("前置完成后几天内的截止不能重复：重复要从一个具体的截止日期往后推", Some("repeat".into()))
}

/// The repeat given, from the due date and on and on unless said.
fn repeat(repeat: Option<RepeatText>, from: Option<RepeatFrom>, until: Option<Date>) -> Result<Option<Repeat>, ApiError> {
    match (repeat, from, until) {
        (None, Some(_), _) => Err(ApiError::invalid("repeat_from 只能和 repeat 一起给", Some("repeat_from".into()))),
        (None, _, Some(_)) => Err(ApiError::invalid("repeat_until 只能和 repeat 一起给", Some("repeat_until".into()))),
        (r, from, until) => Ok(r.map(|r| Repeat { every: r.every, period: r.period, from: from.unwrap_or(RepeatFrom::Due), until })),
    }
}

/// The operations adding `tasks` to `folder` as those of a plan for it: what AI organizing adds. The
/// numbers in it are those of the tasks the model was `shown`, which must still be there.
pub fn plan_into(b: &Board, folder: Option<FolderId>, tasks: Vec<PlanTask>, shown: &HashMap<i64, TaskId>) -> Result<Vec<Op>, ApiError> {
    plan_ops(b, Plan { folder: Some(b.path(folder)), tasks }, &|n| shown.get(&n).copied().filter(|&id| b.task(id).is_some()))
}

/// The user's own tasks by number, as tools take them.
fn mine<'a>(b: &'a Board) -> impl Fn(i64) -> Option<TaskId> + 'a {
    |n| b.task_by_number(b.me(), n).map(|t| t.id)
}

/// The user ids of people in shared folders given by username.
fn people(b: &Board, usernames: &[String]) -> Result<Vec<String>, ApiError> {
    usernames.iter().map(|u| Ok(person(b, u)?.id.clone())).collect()
}

/// The operations a plan amounts to; `number` finds the tasks plain numbers in it stand for.
fn plan_ops(b: &Board, plan: Plan, number: &dyn Fn(i64) -> Option<TaskId>) -> Result<Vec<Op>, ApiError> {
    let mut ops = Vec::new();
    let mut folders = Folders { board: b, made: HashMap::new() };
    let (base, base_path) = folders.ensure(&mut ops, plan.folder.as_deref().unwrap_or("/"))?;
    let ids: Vec<TaskId> = plan.tasks.iter().map(|_| TaskId(Uuid::now_v7())).collect();
    let mut refs = HashMap::new();
    for (t, &id) in plan.tasks.iter().zip(&ids) {
        let Some(name) = &t.name else { continue };
        if task_ref(name).is_some() {
            return Err(ApiError::invalid(format!("ref“{name}”会被识别为事项编号，请改用非数字的名称"), None));
        }
        if refs.insert(name.clone(), id).is_some() {
            return Err(ApiError::invalid(format!("计划中有多个事项的 ref 均为“{name}”"), None));
        }
    }
    let own = |n: i64| number(n).ok_or_else(|| ApiError::not_found(format!("不存在事项 #{n}")));
    let resolve = |r: &PlanRef| -> Result<TaskId, ApiError> {
        match r {
            PlanRef::Number(n) => own(*n),
            PlanRef::Name(s) => match task_ref(s) {
                Some((None, n)) => own(n),
                Some((Some(user), number)) => Ok(find_task(b, &TaskRef { user: Some(user.to_owned()), number })?.id),
                None => refs.get(s).copied().ok_or_else(|| ApiError::invalid(format!("计划中没有 ref 为“{s}”的事项"), None)),
            },
        }
    };
    let mut links = Vec::new();
    for (t, &id) in plan.tasks.into_iter().zip(&ids) {
        let PlanTask {
            name: _,
            title,
            description,
            folder,
            start,
            due: due_text,
            due_after,
            remind,
            repeat: repeat_text,
            repeat_from,
            repeat_until,
            assign,
            after,
            before,
        } = t;
        let folder_id = match folder {
            None => base,
            Some(p) if p.trim_start().starts_with('/') => folders.ensure(&mut ops, &p)?.0,
            Some(p) => folders.ensure(&mut ops, &format!("{base_path}/{p}"))?.0,
        };
        let (start, start_time) = start.map_or((None, None), |s| (Some(s.date), s.time));
        let (due, due_after) = new_due(due_text, due_after, remind, repeat_text, repeat_from, repeat_until)?;
        ops.push(Op::TaskCreate {
            id,
            title,
            folder_id,
            description: description.unwrap_or_default(),
            start,
            start_time,
            due,
            due_after,
            sort_key: None,
            done: false,
            assignees: people(b, &assign)?,
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
        start: a.start,
        due: a.due,
        due_after: a.due_after,
        remind: a.remind,
        repeat: a.repeat,
        repeat_from: a.repeat_from,
        repeat_until: a.repeat_until,
        assign: a.assign,
        after: plan_refs(a.after),
        before: plan_refs(a.before),
    };
    plan_ops(b, Plan { folder: a.folder, tasks: vec![task] }, &mine(b)).map(Write::from)
}

/// Tasks given by number, as a plan has them.
fn plan_refs(refs: Vec<TaskRef>) -> Vec<PlanRef> {
    refs.iter().map(|r| PlanRef::Name(r.to_string())).collect()
}

pub fn apply(w: &WriteCx, a: ApplyArgs) -> Result<Write, ApiError> {
    let b = w.board;
    if a.plan.tasks.is_empty() {
        return Err(ApiError::invalid("计划至少包含一个事项", Some("plan.tasks".into())));
    }
    Ok(Write { ops: plan_ops(b, a.plan, &mine(b))?, preview: a.dry_run })
}

pub fn edit(w: &WriteCx, a: EditArgs) -> Result<Write, ApiError> {
    let t = find_task(w.board, &a.task)?;
    let Edit { id, title, description, start, due, due_after, assign, unassign } = edit_fields(w.board, t, a)?;
    // A start date given without a time starts at the start of its day; cleared, its time goes too.
    let (start, start_time) = match start {
        None => (None, None),
        Some(None) => (Some(None), None),
        Some(Some(s)) => (Some(Some(s.date)), Some(s.time)),
    };
    let op = Op::TaskUpdate {
        id,
        folder_id: None,
        title,
        description,
        start,
        start_time,
        due,
        due_after,
        sort_key: None,
        done: None,
        today: None,
        occurrence: None,
        assign,
        unassign,
    };
    Ok(vec![op].into())
}

/// The fields `edit` sets on a task, the due date in full; `None` where it leaves one as it is; and
/// who is to do it as well or no longer, by user id.
pub(crate) struct Edit {
    pub id: TaskId,
    pub title: Option<Title>,
    pub description: Option<Description>,
    pub start: Option<Option<WhenText>>,
    pub due: Option<Option<Due>>,
    pub due_after: Option<Option<DueAfter>>,
    pub assign: Vec<String>,
    pub unassign: Vec<String>,
}

/// What `edit` changes in task `t`: also what AI tidying suggests changing in a task.
pub(crate) fn edit_fields(b: &Board, t: &Task, a: EditArgs) -> Result<Edit, ApiError> {
    let revising = !a.replace.is_empty() || a.append.is_some();
    let changes_repeat = a.repeat.is_some() || a.repeat_from.is_some() || a.repeat_until.is_some();
    if a.title.is_none()
        && a.description.is_none()
        && !revising
        && a.start.is_none()
        && a.due.is_none()
        && a.due_after.is_none()
        && a.remind.is_none()
        && !changes_repeat
        && a.assign.is_empty()
        && a.unassign.is_empty()
    {
        return Err(ApiError::invalid("未指定要修改的内容", None));
    }
    if a.description.is_some() && revising {
        return Err(ApiError::invalid("description 是整段替换，不能和 replace、append 同时给", Some("description".into())));
    }
    let description = if revising { Some(revise(t.description.as_str(), a.replace, a.append)?) } else { a.description };
    // One due some days after its prerequisites has the date they gave it as its due date, not its own.
    let current = t.due.as_ref().filter(|_| t.due_after.is_none());
    let current_reminders = t.reminders();
    // `Some(empty)` clears them.
    let remind = a.remind.map(|r| r.map_or_else(Vec::new, |r| r.0));
    // `Some(None)` stops it repeating.
    let new_repeat = if changes_repeat { Some(repeat(a.repeat.flatten(), a.repeat_from, a.repeat_until)?) } else { None };
    // Some days after the prerequisites, as given, or as it is with other reminders.
    let after = match (a.due_after, &t.due_after) {
        (Some(_), _) if a.due.is_some() => return Err(mutate::both_due()),
        (Some(after), _) => Some(after.map(|a| (a.days, a.time))),
        (None, Some(cur)) if a.due.is_none() && (remind.is_some() || changes_repeat) => {
            Some(Some((cur.days, cur.at.as_ref().map(|at| at.time.clone()))))
        }
        _ => None,
    };
    if let Some(after) = after {
        if matches!(new_repeat, Some(Some(_))) {
            return Err(repeating_after());
        }
        let due_after = match after {
            None if remind.as_ref().is_some_and(|r| !r.is_empty()) => {
                return Err(ApiError::invalid("清除截止时间时不能同时设置提醒", Some("remind".into())));
            }
            None => None,
            Some((days, time)) => {
                let timed = time.is_some();
                let remind =
                    remind.unwrap_or_else(|| current_reminders.into_iter().filter(|r| timed || matches!(r, Remind::On(_))).collect());
                Some(due_after(days, time, remind)?)
            }
        };
        return Ok(Edit {
            id: t.id,
            title: a.title,
            description,
            start: a.start,
            due: None,
            due_after: (due_after != t.due_after).then_some(due_after),
            assign: people(b, &a.assign)?,
            unassign: people(b, &a.unassign)?,
        });
    }
    let new_due = match (a.due, remind) {
        (None, None) if new_repeat.is_none() => None,
        (Some(None), remind) => {
            if remind.is_some_and(|r| !r.is_empty()) {
                return Err(ApiError::invalid("清除截止时间时不能同时设置提醒", Some("remind".into())));
            }
            if matches!(new_repeat, Some(Some(_))) {
                return Err(ApiError::invalid("清除截止时间时不能同时设置重复", Some("repeat".into())));
            }
            Some(None)
        }
        (Some(Some(d)), remind) => {
            // Moving the deadline keeps the reminders, those before its time while it has one, and the
            // repeat.
            let timed = d.time.is_some();
            let remind = remind.unwrap_or_else(|| current_reminders.into_iter().filter(|r| timed || matches!(r, Remind::On(_))).collect());
            Some(due(Some(d), remind, new_repeat.unwrap_or(current.and_then(|c| c.repeat.clone())))?)
        }
        (None, remind) => match current {
            Some(c) => {
                let time = c.at.as_ref().map(|at| at.time.clone());
                // The date as it is, only a new reminder or repeat can be wrong: the one before it
                // repeats no more, one by the lunar calendar can't go from it, or the reminder.
                let field = match new_repeat.as_ref().and_then(|r| r.as_ref()?.misfit(&c.date)) {
                    Some(Misfit::Late) => "repeat_until",
                    Some(Misfit::OffTable) => "repeat",
                    None => "remind",
                };
                let new = Due::new(c.date.clone(), time, remind.unwrap_or(current_reminders), new_repeat.unwrap_or(c.repeat.clone()))
                    .map_err(|m| ApiError::invalid(m, Some(field.into())))?;
                // Clearing a reminder there isn't changes nothing.
                (Some(&new) != current).then_some(Some(new))
            }
            None if remind.as_ref().is_some_and(|r| !r.is_empty()) => {
                return Err(ApiError::invalid("该事项没有截止时间，无法设置提醒", Some("remind".into())));
            }
            None if matches!(new_repeat, Some(Some(_))) => {
                return Err(ApiError::invalid("该事项没有截止时间，无法设置重复", Some("repeat".into())));
            }
            // Without a due date there is no reminder or repeat, so clearing them changes nothing.
            None => None,
        },
    };
    Ok(Edit {
        id: t.id,
        title: a.title,
        description,
        start: a.start,
        due: new_due,
        due_after: None,
        assign: people(b, &a.assign)?,
        unassign: people(b, &a.unassign)?,
    })
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
fn set_done(b: &Board, tasks: Vec<TaskRef>, done: bool, today: Option<Date>, end_repeat: bool) -> Result<Write, ApiError> {
    if tasks.is_empty() {
        return Err(ApiError::invalid("至少需要指定一个事项", Some("tasks".into())));
    }
    let mut seen = HashSet::new();
    let mut ops = Vec::new();
    for r in &tasks {
        let t = find_task(b, r)?;
        if seen.insert(t.id) {
            let due = t.due.as_ref().filter(|d| end_repeat && d.repeat.is_some()).map(|d| Some(Due { repeat: None, ..d.clone() }));
            ops.push(Op::TaskUpdate {
                id: t.id,
                folder_id: None,
                title: None,
                description: None,
                start: None,
                start_time: None,
                due,
                due_after: None,
                sort_key: None,
                done: Some(done),
                today: today.clone(),
                occurrence: None,
                assign: Vec::new(),
                unassign: Vec::new(),
            });
        }
    }
    Ok(ops.into())
}

pub fn done(w: &WriteCx, a: DoneArgs) -> Result<Write, ApiError> {
    let b = w.board;
    set_done(b, a.tasks, true, w.today.clone(), a.end_repeat)
}

pub fn reopen(w: &WriteCx, a: TasksArgs) -> Result<Write, ApiError> {
    let b = w.board;
    set_done(b, a.tasks, false, None, false)
}

/// Moves repeating tasks on to their next time without counting them done (docs/design.md#重复).
pub fn skip(w: &WriteCx, a: TasksArgs) -> Result<Write, ApiError> {
    if a.tasks.is_empty() {
        return Err(ApiError::invalid("至少需要指定一个事项", Some("tasks".into())));
    }
    let today = w.today.as_ref().ok_or_else(|| ApiError::invalid("跳过重复的事项需要给出今天的日期", Some("today".into())))?;
    let mut seen = HashSet::new();
    let mut ops = Vec::new();
    for r in &a.tasks {
        let t = find_task(w.board, r)?;
        if !seen.insert(t.id) {
            continue;
        }
        let (Some(due @ Due { repeat: Some(repeat), .. }), None) = (&t.due, t.done_at) else {
            return Err(ApiError::conflict(format!("事项 {r} 不是待办的重复事项")));
        };
        // Skipped early, one counted from the day done is counted from its due date: never back.
        let Some(next) = repeat.next(&due.date, today.max(&due.date)) else {
            return Err(ApiError::conflict(format!("事项 {r} 已是最后一次，不能跳过；不再做了就用 done 的 end_repeat 结束重复")));
        };
        ops.push(Op::TaskUpdate {
            id: t.id,
            folder_id: None,
            title: None,
            description: None,
            start: t.start.as_ref().map(|s| Some(s.shifted(&due.date, &next))),
            start_time: None,
            due: Some(Some(Due { date: next, ..due.clone() })),
            due_after: None,
            sort_key: None,
            done: None,
            today: None,
            occurrence: None,
            assign: Vec::new(),
            unassign: Vec::new(),
        });
    }
    Ok(ops.into())
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
    let invalid = |m: String| ApiError::invalid(m, Some("name".into()));
    let op = match (a.folder, a.attachment) {
        (Some(folder), None) => {
            let id = folder_id(w.board, &folder, "改名")?;
            Op::FolderUpdate {
                id,
                name: Some(Name::try_from(a.name).map_err(invalid)?),
                parent_id: None,
                sort_key: None,
                color: None,
                icon: None,
            }
        }
        (None, Some(id)) => Op::AttachmentUpdate { id, name: Some(FileName::try_from(a.name).map_err(invalid)?), task_id: None },
        _ => return Err(ApiError::invalid("folder 和 attachment 给且只给一个", None)),
    };
    Ok(vec![op].into())
}

/// Saves the file, recorded as the user's upload, and attaches it: as `PUT /files/{id}` and
/// `attachment.add` do, in the change's transaction.
pub fn attach(w: &WriteCx, a: AttachArgs) -> Result<Write, ApiError> {
    let t = find_task(w.board, &a.task)?;
    let bytes = BASE64.decode(a.data.as_bytes()).map_err(|_| ApiError::invalid("data 不是有效的 base64", Some("data".into())))?;
    let size = u32::try_from(bytes.len()).map_or(Err("文件不能超过 20 MB"), FileSize::try_from);
    let size = size.map_err(|m| ApiError::invalid(m, Some("data".into())))?;
    let media_type = match a.media_type {
        Some(t) => t,
        None => {
            let guessed = mime_guess::from_path(a.name.as_str()).first_raw().unwrap_or("application/octet-stream");
            MediaType::try_from(guessed.to_owned()).expect("mime_guess gives media types")
        }
    };
    let id = AttachmentId(Uuid::now_v7());
    // Rolled back with the rest when syncing through a drive: the change's commit records it.
    store::insert_file(w.c, id, &StoredFile { owner: w.user.to_owned(), media_type: media_type.clone(), size }, w.now, false)?;
    // Left for the daily sweep when the change fails after this.
    w.files.write(id, &bytes)?;
    Ok(vec![Op::AttachmentAdd { id, name: a.name, media_type, size, task_id: Some(t.id), job_id: None }].into())
}

pub fn detach(_: &WriteCx, a: DetachArgs) -> Result<Write, ApiError> {
    Ok(a.attachments.into_iter().map(|id| Op::AttachmentDelete { id }).collect::<Vec<_>>().into())
}

pub fn move_to(w: &WriteCx, a: MoveArgs) -> Result<Write, ApiError> {
    let b = w.board;
    if a.tasks.is_empty() && a.folders.is_empty() {
        return Err(ApiError::invalid("未指定要移动的事项或文件夹", None));
    }
    let to = find_folder(b, &a.to)?;
    let mut ops = Vec::new();
    for r in &a.tasks {
        let t = find_task(b, r)?;
        ops.push(Op::TaskUpdate {
            id: t.id,
            folder_id: Some(to),
            title: None,
            description: None,
            start: None,
            start_time: None,
            due: None,
            due_after: None,
            sort_key: None,
            done: None,
            today: None,
            occurrence: None,
            assign: Vec::new(),
            unassign: Vec::new(),
        });
    }
    for path in &a.folders {
        ops.push(Op::FolderUpdate {
            id: folder_id(b, path, "移动")?,
            name: None,
            parent_id: Some(to),
            sort_key: None,
            color: None,
            icon: None,
        });
    }
    Ok(ops.into())
}

pub fn style(w: &WriteCx, a: StyleArgs) -> Result<Write, ApiError> {
    if a.color.is_none() && a.icon.is_none() {
        return Err(ApiError::invalid("color 和 icon 至少给一个", None));
    }
    let id = folder_id(w.board, &a.folder, "设颜色和图标")?;
    Ok(vec![Op::FolderUpdate { id, name: None, parent_id: None, sort_key: None, color: a.color, icon: a.icon }].into())
}

/// What a copy of a folder named `name` is called where `taken` says which names are in use: its
/// own name, else "名字 副本", "名字 副本 2" and on (docs/design.md#文件夹). As `copyName` in
/// apps/web/src/domain/copy.ts.
fn copy_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    // A long name is cut short to leave room for what tells the copy apart.
    let copy = |mark: String| name.chars().take(MAX_NAME - mark.chars().count()).chain(mark.chars()).collect::<String>();
    [name.to_owned(), copy(" 副本".into())]
        .into_iter()
        .chain((2..).map(|n| copy(format!(" 副本 {n}"))))
        .find(|n| !taken(n))
        .expect("endless")
}

/// Copies a folder with all in it: subfolders, tasks open again, the dependencies between them, and
/// their attachments, whose files it copies (docs/design.md#文件夹).
pub fn copy(w: &WriteCx, a: CopyArgs) -> Result<Write, ApiError> {
    let b = w.board;
    let from = folder_id(b, &a.folder, "复制")?;
    let source = b.folder(from).expect("found by path");
    let to = match &a.to {
        Some(path) => find_folder(b, path)?,
        // Beside it, or, shared with the user, at their root.
        None if b.shared_with_me().iter().any(|f| f.id == from) => None,
        None => source.parent_id,
    };
    let name = match a.name {
        Some(name) => name,
        None => copy_name(source.name.as_str(), |n| b.subfolders(to).iter().any(|f| f.name.as_str() == n)),
    };
    let name = Name::try_from(name).map_err(|m| ApiError::invalid(m, Some("name".into())))?;
    let shared_root = |f: Option<FolderId>| b.share(f).map(|s| s.folder_id);

    let root = FolderId(Uuid::now_v7());
    let mut ops = vec![Op::FolderCreate { id: root, name, parent_id: to, sort_key: None, color: source.color, icon: source.icon }];
    // Each task with its copy's id and folder, folder by folder.
    let mut tasks: Vec<(&Task, TaskId, FolderId)> = Vec::new();
    let mut stack = vec![(from, root)];
    while let Some((old, new)) = stack.pop() {
        tasks.extend(b.contents(Some(old)).iter().map(|t| (*t, TaskId(Uuid::now_v7()), new)));
        for f in b.subfolders(Some(old)) {
            let id = FolderId(Uuid::now_v7());
            ops.push(Op::FolderCreate {
                id,
                name: f.name.clone(),
                parent_id: Some(new),
                sort_key: Some(f.sort_key.clone()),
                color: f.color,
                icon: f.icon,
            });
            stack.push((f.id, id));
        }
    }
    let copied: HashMap<TaskId, TaskId> = tasks.iter().map(|(t, new, _)| (t.id, *new)).collect();
    let attachments: Vec<(&Attachment, AttachmentId, TaskId)> =
        tasks.iter().flat_map(|(t, new, _)| b.attachments(t).iter().map(|a| (*a, AttachmentId(Uuid::now_v7()), *new))).collect();
    let mut deps = Vec::new();
    for (t, new, folder) in &tasks {
        let mut description = t.description.as_str().to_owned();
        for (a, id, _) in attachments.iter().filter(|(_, _, holder)| holder == new) {
            description = description.replace(&format!("attachment:{}", a.id), &format!("attachment:{id}"));
        }
        ops.push(Op::TaskCreate {
            id: *new,
            title: t.title.clone(),
            folder_id: Some(*folder),
            description: Description::try_from(description).expect("ids as long as before"),
            start: t.start.clone(),
            start_time: t.start_time.clone(),
            // One due after its prerequisites counts anew (docs/design.md#前置完成后的截止).
            due: t.due.clone().filter(|_| t.due_after.is_none()),
            due_after: t.due_after.clone(),
            sort_key: Some(t.sort_key.clone()),
            done: false,
            // Still in the shared folder it was in, a task keeps who it is assigned to.
            assignees: if shared_root(t.folder_id) == shared_root(to) { t.assignees.clone() } else { Vec::new() },
        });
        deps.extend(b.after(t).iter().filter_map(|after| Some(Op::DepAdd { before: *new, after: *copied.get(&after.id)? })));
    }
    ops.extend(deps);
    // Counted before copying any file: too many, there would be nothing to copy them for.
    if ops.len() + attachments.len() > MAX_OPS {
        return Err(ApiError::invalid(format!("文件夹内容过多，一次最多复制 {MAX_OPS} 项（文件夹、事项、依赖和附件合计）"), None));
    }
    for (a, id, task) in attachments {
        // As `attach` does: rolled back with the rest when syncing through a drive, and left for the
        // daily sweep when the change fails after this.
        store::insert_file(
            w.c,
            id,
            &StoredFile { owner: w.user.to_owned(), media_type: a.media_type.clone(), size: a.size },
            w.now,
            false,
        )?;
        w.files.publish(copy_file(w.files, a)?, id)?;
        ops.push(Op::AttachmentAdd {
            id,
            name: a.name.clone(),
            media_type: a.media_type.clone(),
            size: a.size,
            task_id: Some(task),
            job_id: None,
        });
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
    for r in &a.tasks {
        let id = find_task(b, r)?.id;
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
    Ok(vec![Op::DepAdd { before: find_task(b, &a.before)?.id, after: find_task(b, &a.after)?.id }].into())
}

pub fn dep_remove(w: &WriteCx, a: DepArgs) -> Result<Write, ApiError> {
    let b = w.board;
    Ok(vec![Op::DepRemove { before: find_task(b, &a.before)?.id, after: find_task(b, &a.after)?.id }].into())
}

pub fn share(w: &WriteCx, a: ShareArgs) -> Result<Write, ApiError> {
    let folder_id = folder_id(w.board, &a.folder, "共享")?;
    if a.users.is_empty() {
        return Err(ApiError::invalid("至少需要指定一个用户", Some("users".into())));
    }
    Ok(a.users.into_iter().map(|username| Op::ShareAdd { folder_id, username }).collect::<Vec<_>>().into())
}

pub fn unshare(w: &WriteCx, a: UnshareArgs) -> Result<Write, ApiError> {
    let b = w.board;
    let folder_id = folder_id(b, &a.folder, "共享")?;
    let share = b
        .share(Some(folder_id))
        .filter(|s| s.folder_id == folder_id)
        .ok_or_else(|| ApiError::invalid(format!("{} 没有共享", b.path(Some(folder_id))), None))?;
    let users = match (a.users.is_empty(), share.owner == w.user) {
        (true, true) => share.members.clone(),
        (true, false) => vec![w.user.to_owned()],
        (false, _) => people(b, &a.users)?,
    };
    Ok(users.into_iter().map(|user_id| Op::ShareRemove { folder_id, user_id }).collect::<Vec<_>>().into())
}

pub fn undo(w: &WriteCx, a: UndoArgs) -> Result<Write, ApiError> {
    let mutation: MutationId =
        w.c.query_row(
            "SELECT m.id FROM deliveries d JOIN mutations m ON m.seq = d.seq WHERE d.seq = ?1 AND d.user_id = ?2 AND m.effects IS NOT NULL",
            params![a.batch, w.user],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| ApiError::not_found(format!("批次 {} 不存在或已过期", a.batch)))?;
    Ok(vec![Op::Undo { mutation }].into())
}
