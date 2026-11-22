//! The mutation pipeline: validate and apply operations in one transaction, record the resulting
//! entity changes in the mutation log. Resubmitting a mutation id returns the first result.
//!
//! What a user can see and change is their own and what is in the folders shared with them
//! (docs/design.md#共享). Everything in a shared folder is its owner's, so the owner of what is made
//! somewhere is the owner of where it is: the folder's, or at the root the user's. Each part of a
//! change goes to whoever sees it, before or after; someone who sees only one side of it sees
//! something made or deleted.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroU32;

use nexttodo_core::dag::find_path;
use nexttodo_core::model::{
    AiJob, AiJobId, Attachment, AttachmentId, Change, Completion, Completions, Dependency, Due, Effect, Folder, FolderId, Holder, JobKind,
    JobState, Link, MutationId, Person, Share, SortKey, Task, TaskId,
};
use nexttodo_core::ops::{MAX_OPS, Mutation, Op};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::error::ApiError;
use crate::store;

/// Who is making the change.
pub struct Actor<'a> {
    pub user: &'a str,
    pub token_id: &'a str,
    /// API token name, recorded as the source; `None` for the user's own devices.
    pub via: Option<&'a str>,
}

/// A change as its maker sees it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Applied {
    pub seq: i64,
    pub effects: Vec<Effect>,
}

/// A change in the log, and the users it went to, to be told of it once it is committed.
pub struct Recorded {
    pub seq: i64,
    pub to: Vec<String>,
}

pub fn apply(conn: &mut Connection, actor: &Actor, m: &Mutation, now: i64) -> Result<(Applied, Vec<String>), ApiError> {
    let tx = conn.transaction()?;
    if let Some(applied) = earlier(&tx, actor, m.id)? {
        return Ok((applied, Vec::new()));
    }
    let done = run(&tx, actor, &m.ops, now)?;
    let recorded = record(&tx, actor, m.id, &m.ops, &done, now)?;
    tx.commit()?;
    Ok((Applied { seq: recorded.seq, effects: done.view(actor.user) }, recorded.to))
}

/// The result of a mutation that was already applied.
pub fn earlier(c: &Connection, actor: &Actor, id: MutationId) -> Result<Option<Applied>, ApiError> {
    let row: Option<(String, i64, Option<String>)> = c
        .query_row(
            "SELECT m.owner_id, m.seq, coalesce(d.effects, m.effects) FROM mutations m
             LEFT JOIN deliveries d ON d.seq = m.seq AND d.user_id = m.owner_id WHERE m.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((user, seq, effects)) = row else { return Ok(None) };
    if user != actor.user {
        return Err(ApiError::conflict("改动 ID 重复"));
    }
    // past the retention period only the receipt is left
    let effects = effects.map(|e| serde_json::from_str(&e)).transpose()?.unwrap_or_default();
    Ok(Some(Applied { seq, effects }))
}

/// Applies the operations inside the caller's transaction and returns what changed. At most
/// `MAX_OPS` at once, however they were made: one change is one transaction on the one connection.
pub fn run(c: &Connection, actor: &Actor, ops: &[Op], now: i64) -> Result<Done, ApiError> {
    if ops.len() > MAX_OPS {
        return Err(ApiError::invalid(format!("每次最多 {MAX_OPS} 个操作，本次有 {} 个", ops.len()), None));
    }
    let mut cx = Cx::new(c, actor, now);
    for op in ops {
        cx.op(op)?;
    }
    cx.introduce()?;
    Ok(cx.done)
}

/// Adds the mutation to the log, with what each user it concerns sees of it, and returns its
/// sequence number. Those who joined or left a shared folder download everything again from it on.
pub fn record(c: &Connection, actor: &Actor, id: MutationId, ops: &[Op], done: &Done, now: i64) -> Result<Recorded, ApiError> {
    let effects = done.effects();
    c.execute(
        "INSERT INTO mutations (id, owner_id, token_id, actor, ops, effects, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, actor.user, actor.token_id, actor.via, serde_json::to_string(ops)?, serde_json::to_string(&effects)?, now],
    )?;
    let seq = c.last_insert_rowid();
    let mut to = Vec::new();
    for user in done.users(actor.user) {
        let view = done.view(user);
        if view.is_empty() && user != actor.user {
            continue;
        }
        let own = (view != effects).then(|| serde_json::to_string(&view)).transpose()?;
        c.execute("INSERT INTO deliveries (user_id, seq, effects) VALUES (?1, ?2, ?3)", params![user, seq, own])?;
        to.push(user.to_owned());
    }
    for user in &done.resync {
        c.execute("UPDATE users SET pruned_seq = max(pruned_seq, ?2) WHERE id = ?1", params![user, seq])?;
    }
    Ok(Recorded { seq, to })
}

/// Moves a job from state `from` to `to`, as the server does when it starts organizing one and when
/// that fails: a change of its own, with no operations, as nobody asked for it. Returns the job as it
/// is now and the change's sequence number; nothing when the job is gone or no longer `from`.
pub fn set_job_state(
    c: &mut Connection,
    actor: &Actor,
    id: AiJobId,
    from: JobState,
    to: JobState,
    error: Option<String>,
    now: i64,
) -> Result<Option<(AiJob, i64)>, ApiError> {
    change_job(c, actor, id, from, |j| AiJob { state: to, error, ..j }, now)
}

fn change_job(
    c: &mut Connection,
    actor: &Actor,
    id: AiJobId,
    from: JobState,
    change: impl FnOnce(AiJob) -> AiJob,
    now: i64,
) -> Result<Option<(AiJob, i64)>, ApiError> {
    let tx = c.transaction()?;
    let Some((after, done)) = job_change(&tx, actor, id, from, change, now)? else { return Ok(None) };
    let seq = record(&tx, actor, MutationId(Uuid::now_v7()), &[], &done, now)?.seq;
    tx.commit()?;
    Ok(Some((after, seq)))
}

/// Moves job `id` on from state `from` in the caller's transaction, as `change` has it: the job after,
/// and what that did. Nothing when the job is gone or no longer `from`.
pub fn job_change(
    c: &Connection,
    actor: &Actor,
    id: AiJobId,
    from: JobState,
    change: impl FnOnce(AiJob) -> AiJob,
    now: i64,
) -> Result<Option<(AiJob, Done)>, ApiError> {
    let Some(before) = store::load_job(c, actor.user, id)? else { return Ok(None) };
    if before.state != from {
        return Ok(None);
    }
    let mut cx = Cx::new(c, actor, now);
    let after = change(before.clone());
    cx.update_job(before, after.clone(), actor.user)?;
    Ok(Some((after, cx.done)))
}

/// Tells everyone who knows a user that the user's name changed, as a change of its own.
pub fn renamed(c: &Connection, actor: &Actor, before: Person, after: Person, now: i64) -> Result<Recorded, ApiError> {
    let mut users = store::known_by(c, actor.user)?;
    users.push(actor.user.to_owned());
    let mut done = Done::default();
    let side = Side { users, root_of: None };
    done.seen.push(Seen { effect: Effect::Person(Change::Updated { before, after }), before: side.clone(), after: side, kept: false });
    record(c, actor, MutationId(Uuid::now_v7()), &[], &done, now)
}

/// What a change did, and who sees each part of it.
#[derive(Default)]
pub struct Done {
    seen: Vec<Seen>,
    /// Who is to download everything again: those who joined or left a shared folder.
    resync: Vec<String>,
}

/// One part of a change, with who saw it before and who sees it after.
struct Seen {
    effect: Effect,
    before: Side,
    after: Side,
    /// Nothing changed but who sees it: something moved into or out of a shared folder along with
    /// the folder it is in.
    kept: bool,
}

#[derive(Clone, Default)]
struct Side {
    users: Vec<String>,
    /// The owner, when this is a shared folder: the only one who sees where it is; to the others it
    /// is at the top.
    root_of: Option<String>,
}

impl Side {
    fn of(users: Vec<String>) -> Side {
        Side { users, root_of: None }
    }

    fn has(&self, user: &str) -> bool {
        self.users.iter().any(|u| u == user)
    }

    /// The folder as `user` sees it.
    fn folder(&self, f: &Folder, user: &str) -> Folder {
        match &self.root_of {
            Some(owner) if owner != user => Folder { parent_id: None, ..f.clone() },
            _ => f.clone(),
        }
    }
}

impl Done {
    /// A change read from the drive, as its effects were written here: local mode has one user,
    /// who sees all of it (docs/local.md#读入).
    pub(crate) fn replayed(effects: Vec<Effect>, user: &str) -> Done {
        let side = Side::of(vec![user.to_owned()]);
        let seen = effects.into_iter().map(|effect| Seen { effect, before: side.clone(), after: side.clone(), kept: false }).collect();
        Done { seen, resync: Vec::new() }
    }

    /// Everything it did.
    pub fn effects(&self) -> Vec<Effect> {
        self.seen.iter().filter(|s| !s.kept).map(|s| s.effect.clone()).collect()
    }

    /// Everyone it concerns, `actor` too.
    fn users<'a>(&'a self, actor: &'a str) -> BTreeSet<&'a str> {
        let mut users: BTreeSet<&str> =
            self.seen.iter().flat_map(|s| s.before.users.iter().chain(&s.after.users)).map(String::as_str).collect();
        users.insert(actor);
        users
    }

    /// What `user` sees of it.
    pub fn view(&self, user: &str) -> Vec<Effect> {
        self.seen
            .iter()
            .filter_map(|s| {
                let (b, a) = (s.before.has(user), s.after.has(user));
                match &s.effect {
                    Effect::Folder(change) => {
                        let seen = sided(change, b, a, s.kept)?;
                        let masked = match seen {
                            Change::Created(f) => Change::Created(s.after.folder(&f, user)),
                            Change::Deleted(f) => Change::Deleted(s.before.folder(&f, user)),
                            Change::Updated { before, after } => {
                                let (before, after) = (s.before.folder(&before, user), s.after.folder(&after, user));
                                if before == after {
                                    return None;
                                }
                                Change::Updated { before, after }
                            }
                        };
                        Some(Effect::Folder(masked))
                    }
                    Effect::Task(change) => sided(change, b, a, s.kept).map(Effect::Task),
                    Effect::Attachment(change) => sided(change, b, a, s.kept).map(Effect::Attachment),
                    Effect::AiJob(change) => sided(change, b, a, s.kept).map(Effect::AiJob),
                    Effect::Share(change) => sided(change, b, a, s.kept).map(Effect::Share),
                    Effect::Person(change) => sided(change, b, a, s.kept).map(Effect::Person),
                    Effect::Dependency(link) => linked(link, b, a, s.kept).map(Effect::Dependency),
                    Effect::Completion(link) => linked(link, b, a, s.kept).map(Effect::Completion),
                }
            })
            .collect()
    }
}

/// A link as someone sees it who saw it before (`before`) or sees it after (`after`): one `kept`,
/// carried along with its task, comes to whoever sees it now and goes from whoever doesn't.
fn linked<T: Clone>(l: &Link<T>, before: bool, after: bool, kept: bool) -> Option<Link<T>> {
    match (l, kept, before, after) {
        (Link::Added(x), true, false, true) | (Link::Added(x), false, _, true) => Some(Link::Added(x.clone())),
        (Link::Added(x), true, true, false) | (Link::Removed(x), false, true, _) => Some(Link::Removed(x.clone())),
        _ => None,
    }
}

/// A change as someone sees it who saw what it changed (`before`) or sees what it made (`after`).
fn sided<T: Clone>(c: &Change<T>, before: bool, after: bool, kept: bool) -> Option<Change<T>> {
    match (c, before, after) {
        (Change::Updated { .. }, true, true) if kept => None,
        (Change::Updated { before: b, after: a }, true, true) => Some(Change::Updated { before: b.clone(), after: a.clone() }),
        (Change::Updated { before: b, .. }, true, false) => Some(Change::Deleted(b.clone())),
        (Change::Updated { after: a, .. }, false, true) => Some(Change::Created(a.clone())),
        (Change::Created(x), _, true) => Some(Change::Created(x.clone())),
        (Change::Deleted(x), true, _) => Some(Change::Deleted(x.clone())),
        _ => None,
    }
}

/// Where something is, as far as who can see it: its owner's own data, or a folder its owner shares.
#[derive(Clone, PartialEq, Eq)]
struct Space {
    owner: String,
    /// The shared folder.
    root: Option<FolderId>,
    members: Vec<String>,
}

impl Space {
    fn shared(&self) -> bool {
        self.root.is_some()
    }

    fn sees(&self, user: &str) -> bool {
        self.owner == user || self.members.iter().any(|m| m == user)
    }

    fn people(&self) -> Vec<String> {
        std::iter::once(&self.owner).chain(&self.members).cloned().collect()
    }

    fn side(&self) -> Side {
        Side::of(self.people())
    }
}

/// `cur`'s due date with what changed from `b` to `a` set back to `b`, part by part: the date, the
/// time with the reminders, and the repeat. Undoing moving a repeating task on so keeps reminders or
/// a repeat changed since, or cleared since with the due date; without a date to go on, the other
/// parts have nothing to come back to.
fn restore_due(cur: &Option<Due>, b: &Option<Due>, a: &Option<Due>) -> Option<Due> {
    let (Some(b), Some(a)) = (b, a) else { return if b != a { b.clone() } else { cur.clone() } };
    let cur = cur.as_ref();
    let date = if b.date != a.date { Some(&b.date) } else { cur.map(|c| &c.date) };
    let timed = |d: &Due| (d.at.clone(), d.remind_on.clone());
    let (at, remind_on) = if timed(b) != timed(a) { timed(b) } else { cur.map(timed).unwrap_or_default() };
    Some(Due {
        date: date?.clone(),
        at,
        remind_on,
        repeat: if b.repeat != a.repeat { b.repeat.clone() } else { cur.and_then(|c| c.repeat.clone()) },
    })
}

/// `cur` with the people added between `b` and `a` taken off again and those taken off put back,
/// leaving those changed since.
fn restore_people(cur: &[String], b: &[String], a: &[String]) -> Vec<String> {
    let mut people: BTreeSet<String> = cur.iter().filter(|u| !a.contains(u) || b.contains(u)).cloned().collect();
    people.extend(b.iter().filter(|u| !a.contains(u)).cloned());
    people.into_iter().collect()
}

/// How often a task was done before a change and after it.
type Counted = (Option<Completions>, Option<Completions>);

/// Whether a change that took how often a task was done from `before` to `after` is still the last
/// that counts, as `later`, how changes since took it, have it: each since took back the one before
/// it that still counted, or counted itself (docs/design.md#改动记录和撤销).
fn still_last<T: PartialEq>(before: &T, after: &T, later: Vec<(T, T)>) -> bool {
    let changes: Vec<(&T, &T)> = std::iter::once((before, after)).chain(later.iter().map(|(b, a)| (b, a))).collect();
    let mut counting: Vec<usize> = Vec::new();
    for (i, (b, a)) in changes.iter().enumerate() {
        match counting.last() {
            Some(&top) if changes[top] == (*a, *b) => _ = counting.pop(),
            _ => counting.push(i),
        }
    }
    counting == [0]
}

/// A repeating task is never done: done, it moves on (docs/design.md#重复); nor due where it can't repeat
/// from, which `Due` takes care of as given, and an undo may come to. It does not start after it is due,
/// a due date without a time lasting all day (docs/design.md#开始时间).
fn check_task(t: &Task) -> Result<(), ApiError> {
    if t.done_at.is_some() && t.due.as_ref().is_some_and(|d| d.repeat.is_some()) {
        return Err(ApiError::invalid(format!("“{}”已完成，不能设置重复，请先重新打开", t.title.as_str()), Some("due".into())));
    }
    if let Some(d) = &t.due
        && let Some(m) = d.repeat.as_ref().and_then(|r| r.misfit(&d.date))
    {
        return Err(ApiError::invalid(format!("“{}”的{m}", t.title.as_str()), Some("due".into())));
    }
    let Some(start) = &t.start else {
        return match t.start_time {
            Some(_) => Err(ApiError::invalid("设置开始时刻须先有开始日期", Some("start_time".into()))),
            None => Ok(()),
        };
    };
    let Some(due) = &t.due else { return Ok(()) };
    if *start > due.date {
        return Err(ApiError::invalid(format!("“{}”的开始日期不能晚于截止日期", t.title.as_str()), Some("start".into())));
    }
    if *start == due.date
        && let (Some(time), Some(at)) = (&t.start_time, &due.at)
        && *time > at.time
    {
        return Err(ApiError::invalid(format!("“{}”的开始时间不能晚于截止时间", t.title.as_str()), Some("start_time".into())));
    }
    Ok(())
}

/// The people `users` may be assigned to a task in `space`: some of those it is shared with.
fn check_assignees(space: &Space, users: &[String]) -> Result<(), ApiError> {
    if users.is_empty() {
        return Ok(());
    }
    if !space.shared() {
        return Err(ApiError::invalid("只有共享的文件夹里的事项能设负责人", Some("assignees".into())));
    }
    if !users.iter().all(|u| space.sees(u)) {
        return Err(ApiError::invalid("负责人须是这个文件夹的共享人或成员", Some("assignees".into())));
    }
    Ok(())
}

macro_rules! restore {
    ($cur:expr, $before:expr, $after:expr, $($field:ident),*) => {{
        let mut restored = $cur.clone();
        $(if $before.$field != $after.$field {
            restored.$field = $before.$field.clone();
        })*
        restored
    }};
}

struct Cx<'a> {
    c: &'a Connection,
    user: &'a str,
    via: Option<String>,
    now: i64,
    done: Done,
    /// The shared folder each folder looked at is in, known so far in this change.
    roots: RefCell<HashMap<FolderId, Option<FolderId>>>,
    /// While undoing a change, the numbers its tasks had before it: those carried back along with a
    /// folder to someone else's get theirs again, when still free there.
    numbers: HashMap<TaskId, i64>,
}

impl<'a> Cx<'a> {
    fn new(c: &'a Connection, actor: &Actor<'a>, now: i64) -> Self {
        Cx {
            c,
            user: actor.user,
            via: actor.via.map(str::to_owned),
            now,
            done: Done::default(),
            roots: RefCell::default(),
            numbers: HashMap::new(),
        }
    }

    fn push(&mut self, effect: Effect, before: Side, after: Side, kept: bool) {
        self.done.seen.push(Seen { effect, before, after, kept });
    }

    /// Those a task comes into view for meet who made it and who changed it last, when those are not
    /// among the people who see it: someone who has left, or who shares another folder it came
    /// from. Everyone who sees a task otherwise knows the others (docs/api.md#共享).
    fn introduce(&mut self) -> Result<(), ApiError> {
        let mut met: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for s in &self.done.seen {
            let Effect::Task(Change::Created(t) | Change::Updated { after: t, .. }) = &s.effect else { continue };
            let new = s.after.users.iter().filter(|u| !s.before.has(u));
            for author in [&t.created_by, &t.updated_by].into_iter().filter(|a| !s.after.has(a)) {
                met.entry(author.clone()).or_default().extend(new.clone().cloned());
            }
        }
        for (id, users) in met.into_iter().filter(|(_, users)| !users.is_empty()) {
            let person = store::person(self.c, &id)?;
            self.push(Effect::Person(Change::Created(person)), Side::default(), Side::of(users.into_iter().collect()), false);
        }
        Ok(())
    }

    fn op(&mut self, op: &Op) -> Result<(), ApiError> {
        match op {
            Op::FolderCreate { id, name, parent_id, sort_key, color, icon } => {
                if store::folder_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("文件夹 ID 已存在"));
                }
                let owner = self.owner_in(*parent_id)?;
                let sort_key = match sort_key {
                    Some(k) => k.clone(),
                    None => SortKey::after(store::last_folder_key(self.c, &owner, *parent_id)?.as_ref()),
                };
                let f = Folder {
                    id: *id,
                    parent_id: *parent_id,
                    name: name.clone(),
                    sort_key,
                    color: *color,
                    icon: *icon,
                    created_at: self.now,
                    updated_at: self.now,
                };
                self.create_folder(f, &owner)
            }
            Op::FolderUpdate { id, name, parent_id, sort_key, color, icon } => {
                let (before, owner) = self.folder(*id)?;
                let mut f = before.clone();
                if let Some(name) = name {
                    f.name = name.clone();
                }
                if let Some(color) = color {
                    f.color = *color;
                }
                if let Some(icon) = icon {
                    f.icon = *icon;
                }
                if let Some(parent_id) = parent_id {
                    f.parent_id = *parent_id;
                }
                if let Some(k) = sort_key {
                    f.sort_key = k.clone();
                }
                self.update_folder(before, &owner, f)
            }
            Op::FolderDelete { id } => {
                let (_, owner) = self.folder(*id)?;
                self.delete_folder(*id, &owner)
            }
            Op::TaskCreate { id, title, folder_id, description, start, start_time, due, sort_key, done, assignees } => {
                if store::task_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("事项 ID 已存在"));
                }
                let owner = self.owner_in(*folder_id)?;
                let mut assignees = assignees.clone();
                assignees.sort();
                assignees.dedup();
                check_assignees(&self.space(&owner, *folder_id)?, &assignees)?;
                let sort_key = match sort_key {
                    Some(k) => k.clone(),
                    None => SortKey::after(store::last_task_key(self.c, &owner, *folder_id)?.as_ref()),
                };
                let task = Task {
                    id: *id,
                    number: store::take_task_number(self.c, &owner)?,
                    folder_id: *folder_id,
                    title: title.clone(),
                    description: description.clone(),
                    done_at: done.then_some(self.now),
                    start: start.clone(),
                    start_time: start_time.clone(),
                    due: due.clone(),
                    completions: None,
                    sort_key,
                    assignees,
                    created_at: self.now,
                    updated_at: self.now,
                    created_by: self.user.to_owned(),
                    updated_by: self.user.to_owned(),
                    created_via: self.via.clone(),
                    updated_via: self.via.clone(),
                };
                self.create_task(task, &owner)
            }
            Op::TaskUpdate {
                id,
                folder_id,
                title,
                description,
                start,
                start_time,
                due,
                sort_key,
                done,
                today,
                occurrence,
                assign,
                unassign,
            } => {
                let (before, owner) = self.task(*id)?;
                let mut t = before.clone();
                if let Some(folder_id) = folder_id {
                    t.folder_id = *folder_id;
                }
                if let Some(title) = title {
                    t.title = title.clone();
                }
                if let Some(description) = description {
                    t.description = description.clone();
                }
                if let Some(start) = start {
                    // The time goes with the date; a new date keeps it.
                    t.start_time = t.start_time.take().filter(|_| start.is_some());
                    t.start = start.clone();
                }
                if let Some(time) = start_time {
                    t.start_time = time.clone();
                }
                if let Some(due) = due {
                    t.due = due.clone();
                }
                if let Some(k) = sort_key {
                    t.sort_key = k.clone();
                }
                if !assign.is_empty() || !unassign.is_empty() {
                    let target = if t.folder_id == before.folder_id { owner.clone() } else { self.owner_in(t.folder_id)? };
                    check_assignees(&self.space(&target, t.folder_id)?, assign)?;
                    let mut people: BTreeSet<String> = t.assignees.iter().chain(assign).cloned().collect();
                    people.retain(|u| !unassign.contains(u));
                    t.assignees = people.into_iter().collect();
                }
                let mut record = None;
                // Done from a list behind the task: that time of a repeating one is done already, or
                // gone (docs/api.md#改动).
                let behind = occurrence.as_ref().is_some_and(|o| t.occurrence().is_some_and(|now| now != *o));
                if let Some(done) = done
                    && *done != t.done_at.is_some()
                    && !(*done && behind)
                {
                    match t.due.as_mut().map(|d| (d.repeat.clone(), d)) {
                        // Done, a repeating task stays open, due next time; done the last time, it is
                        // done (docs/design.md#重复).
                        Some((Some(r), d)) if *done => {
                            let today = today
                                .as_ref()
                                .ok_or_else(|| ApiError::invalid("完成重复的事项需要给出今天的日期", Some("today".into())))?;
                            let again = store::next_again(self.c, t.id, &d.date)?;
                            record = Some(Completion { task_id: t.id, due: d.date.clone(), done: today.clone(), again });
                            match r.next(&d.date, today) {
                                Some(next) => {
                                    // The start date keeps as far before it, at the same time (docs/design.md#开始时间).
                                    t.start = t.start.as_ref().map(|s| s.shifted(&d.date, &next));
                                    d.date = next;
                                }
                                None => {
                                    d.repeat = None;
                                    t.done_at = Some(self.now);
                                }
                            }
                            let count = t.completions.as_ref().map_or(NonZeroU32::MIN, |c| c.count.saturating_add(1));
                            t.completions = Some(Completions { count, last: today.clone() });
                        }
                        _ => t.done_at = done.then_some(self.now),
                    }
                }
                self.update_task(before, &owner, t, None)?;
                match record {
                    Some(r) => self.record_completion(r),
                    None => Ok(()),
                }
            }
            Op::TaskDelete { id } => {
                self.task(*id)?;
                self.delete_task(*id)
            }
            Op::DepAdd { before, after } => {
                if before == after {
                    return Err(ApiError::invalid("事项不能依赖自己", None));
                }
                let (b, b_owner) = self.task(*before)?;
                let (a, a_owner) = self.task(*after)?;
                let space = self.space(&b_owner, b.folder_id)?;
                if space != self.space(&a_owner, a.folder_id)? {
                    return Err(ApiError::invalid("只能连同一处的事项：私人的事项之间，或同一个共享的文件夹里的事项之间", None));
                }
                if store::load_dependency(self.c, *before, *after)?.is_some() {
                    return Err(ApiError::conflict("该依赖已存在"));
                }
                self.create_dependency(Dependency { before: *before, after: *after, created_at: self.now }, &space)
            }
            Op::DepRemove { before, after } => {
                let (b, owner) = self.task(*before)?;
                let d = store::load_dependency(self.c, *before, *after)?.ok_or_else(|| ApiError::not_found("该依赖不存在"))?;
                let space = self.space(&owner, b.folder_id)?;
                self.delete_dependency(d, &space)
            }
            Op::AttachmentAdd { id, name, media_type, size, task_id, job_id } => {
                let holder = match (task_id, job_id) {
                    (Some(t), None) => Holder::Task(*t),
                    (None, Some(j)) => Holder::Job(*j),
                    _ => return Err(ApiError::invalid("附件要属于一个事项或一条 AI 整理请求：task_id 和 job_id 给且只给一个", None)),
                };
                let side = self.holder_side(holder)?;
                match store::load_file(self.c, *id)? {
                    Some(f) if f.owner == self.user => {
                        if store::attachment_id_taken(self.c, *id)? {
                            return Err(ApiError::conflict("附件 ID 已存在"));
                        }
                        if f.media_type != *media_type || f.size != *size {
                            return Err(ApiError::invalid("附件的类型或大小和上传的文件不一致", None));
                        }
                    }
                    _ => return Err(ApiError::file_missing()),
                }
                let a =
                    Attachment { id: *id, holder, name: name.clone(), media_type: media_type.clone(), size: *size, created_at: self.now };
                self.create_attachment(a, side)
            }
            Op::AttachmentUpdate { id, name, task_id } => {
                let (before, side) = self.attachment(*id)?;
                let mut a = before.clone();
                if let Some(name) = name {
                    a.name = name.clone();
                }
                if let Some(t) = task_id {
                    a.holder = Holder::Task(*t);
                }
                self.update_attachment(before, side, a)
            }
            Op::AttachmentDelete { id } => {
                let (a, side) = self.attachment(*id)?;
                self.delete_attachment(a, side)
            }
            Op::AiJobCreate { id, kind, text, local_time, folder_id } => {
                if store::job_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("请求 ID 已存在"));
                }
                if let Some(f) = folder_id {
                    self.folder(*f)?;
                }
                self.create_job(AiJob {
                    id: *id,
                    folder_id: *folder_id,
                    kind: kind.unwrap_or_default(),
                    text: text.clone(),
                    local_time: local_time.clone(),
                    state: JobState::Waiting,
                    error: None,
                    suggestions: None,
                    created_at: self.now,
                    updated_at: self.now,
                })
            }
            Op::AiJobRetry { id, text, folder_id } => {
                let before = self.job(*id)?;
                match before.state {
                    JobState::Failed | JobState::Ready => {}
                    JobState::Running => return Err(ApiError::conflict("正在整理，稍后再改")),
                    JobState::Waiting => return Err(ApiError::conflict("这条请求已在等待整理")),
                }
                let mut j = AiJob { state: JobState::Waiting, error: None, suggestions: None, ..before.clone() };
                if let Some(text) = text {
                    j.text = text.clone();
                }
                if let Some(folder_id) = folder_id {
                    if let Some(f) = folder_id {
                        self.folder(*f)?;
                    }
                    j.folder_id = *folder_id;
                }
                let user = self.user;
                self.update_job(before, j, user)
            }
            Op::AiJobDelete { id } => {
                let j = self.job(*id)?;
                let user = self.user;
                self.delete_job(j, user)
            }
            Op::ShareAdd { folder_id, username } => {
                let (f, owner) = self.folder(*folder_id)?;
                if owner != self.user {
                    return Err(ApiError::forbidden("只有共享人能添加成员"));
                }
                let person =
                    store::person_named(self.c, username)?.ok_or_else(|| ApiError::not_found(format!("没有用户“{}”", username.trim())))?;
                if person.id == owner {
                    return Err(ApiError::invalid("不能共享给自己", Some("username".into())));
                }
                self.add_member(&f, &owner, person)
            }
            Op::ShareRemove { folder_id, user_id } => {
                let (f, owner) = self.folder(*folder_id)?;
                if self.user != owner && self.user != user_id {
                    return Err(ApiError::forbidden("只有共享人能移除成员"));
                }
                self.remove_member(&f, &owner, user_id)
            }
            Op::Undo { mutation } => self.undo(*mutation),
        }
    }

    /// The shared folder `id` is in, or is.
    fn root(&self, id: FolderId) -> Result<Option<FolderId>, ApiError> {
        if let Some(root) = self.roots.borrow().get(&id) {
            return Ok(*root);
        }
        let root = store::shared_root(self.c, id)?;
        self.roots.borrow_mut().insert(id, root);
        Ok(root)
    }

    /// Where something of `owner`'s in `folder` is.
    fn space(&self, owner: &str, folder: Option<FolderId>) -> Result<Space, ApiError> {
        let root = match folder {
            Some(f) => self.root(f)?,
            None => None,
        };
        let members = match root {
            Some(r) => store::members(self.c, r)?,
            None => Vec::new(),
        };
        Ok(Space { owner: owner.to_owned(), root, members })
    }

    /// A folder the user can see, and its owner.
    fn folder(&self, id: FolderId) -> Result<(Folder, String), ApiError> {
        let found = store::load_folder(self.c, id)?;
        match found {
            Some((f, owner)) if owner == self.user || self.space(&owner, Some(id))?.sees(self.user) => Ok((f, owner)),
            _ => Err(ApiError::not_found("文件夹不存在")),
        }
    }

    /// A task the user can see, and its owner.
    fn task(&self, id: TaskId) -> Result<(Task, String), ApiError> {
        let found = store::load_task(self.c, id)?;
        match found {
            Some((t, owner)) if owner == self.user || self.space(&owner, t.folder_id)?.sees(self.user) => Ok((t, owner)),
            _ => Err(ApiError::not_found("事项不存在")),
        }
    }

    fn job(&self, id: AiJobId) -> Result<AiJob, ApiError> {
        store::load_job(self.c, self.user, id)?.ok_or_else(|| ApiError::not_found("AI 整理请求不存在"))
    }

    /// Who sees what an attachment belongs to, which the user has to see: a task, or their own job.
    fn holder_side(&self, holder: Holder) -> Result<Side, ApiError> {
        match holder {
            Holder::Task(t) => {
                let (t, owner) = self.task(t)?;
                Ok(self.space(&owner, t.folder_id)?.side())
            }
            Holder::Job(j) => {
                self.job(j)?;
                Ok(Side::of(vec![self.user.to_owned()]))
            }
        }
    }

    /// An attachment the user can see, and who sees it.
    fn attachment(&self, id: AttachmentId) -> Result<(Attachment, Side), ApiError> {
        self.visible_attachment(id)?.ok_or_else(|| ApiError::not_found("附件不存在"))
    }

    /// Whose what is put in `folder` is: the folder's owner's, or at the root the user's.
    fn owner_in(&self, folder: Option<FolderId>) -> Result<String, ApiError> {
        match folder {
            Some(f) => Ok(self.folder(f)?.1),
            None => Ok(self.user.to_owned()),
        }
    }

    /// Who sees a folder, it being in `space`.
    fn folder_side(f: &Folder, space: &Space) -> Side {
        Side { users: space.people(), root_of: (space.root == Some(f.id)).then(|| space.owner.clone()) }
    }

    fn check_name(&self, owner: &str, f: &Folder) -> Result<(), ApiError> {
        if store::folder_name_taken(self.c, owner, f)? {
            return Err(ApiError::conflict(format!("同一层级已有名为“{}”的文件夹", f.name.as_str())));
        }
        Ok(())
    }

    fn create_folder(&mut self, f: Folder, owner: &str) -> Result<(), ApiError> {
        self.check_name(owner, &f)?;
        let space = self.space(owner, f.parent_id)?;
        store::insert_folder(self.c, owner, &f)?;
        self.roots.borrow_mut().insert(f.id, space.root);
        self.push(Effect::Folder(Change::Created(f)), Side::default(), space.side(), false);
        Ok(())
    }

    /// Changes a folder. One moved into or out of a shared folder takes along everything in it; its
    /// tasks lose the dependencies on what stays behind, and the assignees who can't see them now.
    fn update_folder(&mut self, before: Folder, owner: &str, mut after: Folder) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        let from = self.space(owner, Some(before.id))?;
        let shared = from.root == Some(before.id);
        if shared && owner != self.user {
            return Err(ApiError::forbidden("只有共享人能改名、移动或删除共享的文件夹"));
        }
        let (new_owner, to) = if after.parent_id == before.parent_id {
            (owner.to_owned(), from.clone())
        } else {
            let new_owner = self.owner_in(after.parent_id)?;
            if let Some(p) = after.parent_id
                && store::in_subtree(self.c, after.id, p)?
            {
                return Err(ApiError::folder_cycle());
            }
            let there = self.space(&new_owner, after.parent_id)?;
            if there.shared() && !store::shared_within(self.c, before.id)?.is_empty() {
                return Err(ApiError::invalid("共享的文件夹不能放进另一个共享的文件夹", None));
            }
            (new_owner, if shared { from.clone() } else { there })
        };
        if after.parent_id != before.parent_id || after.name != before.name {
            self.check_name(&new_owner, &after)?;
        }
        if shared && after.name != before.name && store::shared_name_taken(self.c, owner, after.name.as_str(), after.id)? {
            return Err(ApiError::conflict(format!("已共享过名为“{}”的文件夹", after.name.as_str())));
        }
        let before_side = Self::folder_side(&before, &from);
        let across = to != from;
        if across {
            self.cut_off(before.id, &from)?;
        }
        after.updated_at = self.now;
        store::update_folder(self.c, &after)?;
        if new_owner != owner {
            store::give_subtree(self.c, after.id, &new_owner)?;
        }
        self.roots.borrow_mut().clear();
        if across {
            self.carry(after.id, &from, &to)?;
        }
        let after_side = Self::folder_side(&after, &to);
        self.push(Effect::Folder(Change::Updated { before, after }), before_side, after_side, false);
        Ok(())
    }

    /// Removes the dependencies between the tasks in the subtree rooted at `id` and those outside it.
    fn cut_off(&mut self, id: FolderId, space: &Space) -> Result<(), ApiError> {
        let inside: BTreeSet<TaskId> = store::subtree_tasks(self.c, id)?.into_iter().map(|t| t.id).collect();
        for d in store::subtree_dependencies(self.c, id)? {
            if !(inside.contains(&d.before) && inside.contains(&d.after)) {
                self.delete_dependency(d, space)?;
            }
        }
        Ok(())
    }

    /// Makes what is in the subtree rooted at `id`, moved from `from` to `to`, belong there: its
    /// tasks to `to`'s owner, numbered anew if that is someone else, assigned only to people there;
    /// AI jobs of people who no longer see their folder go to their root, or go, if tidying it. The
    /// folders and the dependencies inside stay as they are, for those who now see them or don't.
    fn carry(&mut self, id: FolderId, from: &Space, to: &Space) -> Result<(), ApiError> {
        for t in store::subtree_tasks(self.c, id)? {
            let mut after = t.clone();
            after.assignees.retain(|u| to.shared() && to.sees(u));
            // Its new number may be the one it had: it is someone else's still.
            let kept = after == t && to.owner == from.owner;
            if to.owner != from.owner {
                after.number = self.number(&to.owner, t.id, self.numbers.get(&t.id).copied())?;
            }
            if !kept {
                store::update_task(self.c, &to.owner, &after)?;
            }
            self.push(Effect::Task(Change::Updated { before: Box::new(t), after: Box::new(after) }), from.side(), to.side(), kept);
        }
        for a in store::subtree_attachments(self.c, id)? {
            self.push(Effect::Attachment(Change::Updated { before: a.clone(), after: a }), from.side(), to.side(), true);
        }
        for (j, owner) in store::subtree_jobs(self.c, id)? {
            if !to.sees(&owner) {
                self.drop_job(j, &owner)?;
            }
        }
        for f in store::subtree_folders(self.c, id)? {
            if f.id != id {
                self.push(Effect::Folder(Change::Updated { before: f.clone(), after: f }), from.side(), to.side(), true);
            }
        }
        for d in store::subtree_dependencies(self.c, id)? {
            self.push(Effect::Dependency(Link::Added(d)), from.side(), to.side(), true);
        }
        for r in store::subtree_completions(self.c, id)? {
            self.push(Effect::Completion(Link::Added(r)), from.side(), to.side(), true);
        }
        Ok(())
    }

    /// Takes a job of `owner`'s out of a folder they no longer see: organizing goes on at their root,
    /// tidying it is over.
    fn drop_job(&mut self, j: AiJob, owner: &str) -> Result<(), ApiError> {
        match j.kind {
            JobKind::Organize => self.update_job(j.clone(), AiJob { folder_id: None, ..j }, owner),
            JobKind::Tidy => self.delete_job(j, owner),
        }
    }

    /// Records the removal of everything in the subtree (dependencies, attachments, completion records,
    /// tasks, AI jobs, folders deepest first)
    /// so that undo can bring it all back in reverse order. Folders are deleted one by one, deepest
    /// first, because a single cascade through a deep tree would hit SQLite's trigger depth limit.
    /// Shared folders inside go first, as their members see them. A shared folder stops being
    /// shared first, its tasks assigned to nobody before, so that bringing it back shares it again
    /// and then assigns them again.
    fn delete_folder(&mut self, id: FolderId, owner: &str) -> Result<(), ApiError> {
        let space = self.space(owner, Some(id))?;
        if space.root == Some(id) {
            if owner != self.user {
                return Err(ApiError::forbidden("只有共享人能改名、移动或删除共享的文件夹"));
            }
            for t in store::subtree_tasks(self.c, id)?.into_iter().filter(|t| !t.assignees.is_empty()) {
                let after = Task { assignees: Vec::new(), ..t.clone() };
                store::update_task(self.c, owner, &after)?;
                self.push(Effect::Task(Change::Updated { before: Box::new(t), after: Box::new(after) }), space.side(), space.side(), false);
            }
            let share = Share { folder_id: id, owner: owner.to_owned(), members: space.members.clone() };
            self.push(Effect::Share(Change::Deleted(share)), space.side(), Side::default(), false);
        } else if space.root.is_none() {
            for shared in store::shared_within(self.c, id)? {
                self.delete_folder(shared, owner)?;
            }
        }
        for d in store::subtree_dependencies(self.c, id)? {
            self.push(Effect::Dependency(Link::Removed(d)), space.side(), Side::default(), false);
        }
        for a in store::subtree_attachments(self.c, id)? {
            self.delete_attachment(a, space.side())?;
        }
        for r in store::subtree_completions(self.c, id)? {
            self.push(Effect::Completion(Link::Removed(r)), space.side(), Side::default(), false);
        }
        for t in store::subtree_tasks(self.c, id)? {
            self.push(Effect::Task(Change::Deleted(Box::new(t))), space.side(), Side::default(), false);
        }
        for (j, owner) in store::subtree_jobs(self.c, id)? {
            self.delete_job(j, &owner)?;
        }
        for f in store::subtree_folders(self.c, id)? {
            store::delete_folder(self.c, f.id)?;
            let side = Self::folder_side(&f, &space);
            self.push(Effect::Folder(Change::Deleted(f)), side, Side::default(), false);
        }
        self.roots.borrow_mut().clear();
        Ok(())
    }

    fn create_task(&mut self, mut t: Task, owner: &str) -> Result<(), ApiError> {
        check_task(&t)?;
        let space = self.space(owner, t.folder_id)?;
        t.assignees.retain(|u| space.shared() && space.sees(u));
        store::insert_task(self.c, owner, &t)?;
        self.push(Effect::Task(Change::Created(Box::new(t))), Side::default(), space.side(), false);
        Ok(())
    }

    /// Changes a task. One moved into or out of a shared folder loses its dependencies, which were
    /// all on what stays behind, and the assignees who can't see it now, takes its attachments along
    /// for who sees it now, and belongs to the owner of
    /// where it is: numbered anew if that is someone else, or with the number it had there, when that
    /// is `had`, undoing such a move, and still free.
    fn update_task(&mut self, before: Task, owner: &str, mut after: Task, had: Option<i64>) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        check_task(&after)?;
        let from = self.space(owner, before.folder_id)?;
        let (new_owner, to) = if after.folder_id == before.folder_id {
            (owner.to_owned(), from.clone())
        } else {
            let new_owner = self.owner_in(after.folder_id)?;
            let to = self.space(&new_owner, after.folder_id)?;
            (new_owner, to)
        };
        if to != from {
            for d in store::task_dependencies(self.c, before.id)? {
                self.delete_dependency(d, &from)?;
            }
        }
        after.assignees.retain(|u| to.shared() && to.sees(u));
        if new_owner != owner {
            after.number = self.number(&new_owner, after.id, had)?;
        }
        after.updated_at = self.now;
        after.updated_by = self.user.to_owned();
        after.updated_via = self.via.clone();
        store::update_task(self.c, &new_owner, &after)?;
        let id = after.id;
        self.push(Effect::Task(Change::Updated { before: Box::new(before), after: Box::new(after) }), from.side(), to.side(), false);
        if to != from {
            for a in store::task_attachments(self.c, id)? {
                self.push(Effect::Attachment(Change::Updated { before: a.clone(), after: a }), from.side(), to.side(), true);
            }
            for r in store::task_completions(self.c, id)? {
                self.push(Effect::Completion(Link::Added(r)), from.side(), to.side(), true);
            }
        }
        Ok(())
    }

    /// The number task `id` gets as it goes to `owner`: `had`, the one it had there, undoing the
    /// change that took it away, when `owner` still has it free; otherwise the next one.
    fn number(&self, owner: &str, id: TaskId, had: Option<i64>) -> Result<i64, ApiError> {
        match had {
            Some(n) if store::number_free(self.c, owner, n, id)? => Ok(n),
            _ => Ok(store::take_task_number(self.c, owner)?),
        }
    }

    fn delete_task(&mut self, id: TaskId) -> Result<(), ApiError> {
        let (t, owner) = self.task(id)?;
        let space = self.space(&owner, t.folder_id)?;
        for d in store::task_dependencies(self.c, id)? {
            self.push(Effect::Dependency(Link::Removed(d)), space.side(), Side::default(), false);
        }
        for a in store::task_attachments(self.c, id)? {
            self.delete_attachment(a, space.side())?;
        }
        for r in store::task_completions(self.c, id)? {
            self.push(Effect::Completion(Link::Removed(r)), space.side(), Side::default(), false);
        }
        store::delete_task(self.c, id)?;
        self.push(Effect::Task(Change::Deleted(Box::new(t))), space.side(), Side::default(), false);
        Ok(())
    }

    fn create_job(&mut self, j: AiJob) -> Result<(), ApiError> {
        store::insert_job(self.c, self.user, &j)?;
        let side = Side::of(vec![self.user.to_owned()]);
        self.push(Effect::AiJob(Change::Created(Box::new(j))), Side::default(), side, false);
        Ok(())
    }

    /// Changes a job of `owner`'s.
    fn update_job(&mut self, before: AiJob, mut after: AiJob, owner: &str) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        after.updated_at = self.now;
        store::update_job(self.c, &after)?;
        let side = Side::of(vec![owner.to_owned()]);
        self.push(Effect::AiJob(Change::Updated { before: Box::new(before), after: Box::new(after) }), side.clone(), side, false);
        Ok(())
    }

    fn delete_job(&mut self, j: AiJob, owner: &str) -> Result<(), ApiError> {
        for a in store::job_attachments(self.c, j.id)? {
            self.delete_attachment(a, Side::of(vec![owner.to_owned()]))?;
        }
        store::delete_job(self.c, j.id)?;
        self.push(Effect::AiJob(Change::Deleted(Box::new(j))), Side::of(vec![owner.to_owned()]), Side::default(), false);
        Ok(())
    }

    fn create_attachment(&mut self, a: Attachment, side: Side) -> Result<(), ApiError> {
        store::insert_attachment(self.c, &a)?;
        self.push(Effect::Attachment(Change::Created(a)), Side::default(), side, false);
        Ok(())
    }

    /// Renames an attachment, or moves it to what the user sees too.
    fn update_attachment(&mut self, before: Attachment, side: Side, after: Attachment) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        let after_side = if after.holder == before.holder { side.clone() } else { self.holder_side(after.holder)? };
        store::update_attachment(self.c, &after)?;
        self.push(Effect::Attachment(Change::Updated { before, after }), side, after_side, false);
        Ok(())
    }

    fn delete_attachment(&mut self, a: Attachment, side: Side) -> Result<(), ApiError> {
        store::delete_attachment(self.c, a.id, self.now)?;
        self.push(Effect::Attachment(Change::Deleted(a)), side, Side::default(), false);
        Ok(())
    }

    fn create_dependency(&mut self, d: Dependency, space: &Space) -> Result<(), ApiError> {
        let edges = store::dependency_edges(self.c, &space.owner)?;
        if let Some(path) = find_path(&edges, d.after, d.before) {
            let path: Vec<TaskId> = [d.before].into_iter().chain(path).collect();
            let titles =
                path.iter().map(|&id| Ok(format!("“{}”", self.task(id)?.0.title.as_str()))).collect::<Result<Vec<_>, ApiError>>()?;
            return Err(ApiError::cycle(path, &titles.join(" → ")));
        }
        store::insert_dependency(self.c, &space.owner, &d)?;
        self.push(Effect::Dependency(Link::Added(d)), Side::default(), space.side(), false);
        Ok(())
    }

    fn delete_dependency(&mut self, d: Dependency, space: &Space) -> Result<(), ApiError> {
        store::delete_dependency(self.c, &d)?;
        self.push(Effect::Dependency(Link::Removed(d)), space.side(), Side::default(), false);
        Ok(())
    }

    /// Records a time its task was done.
    fn record_completion(&mut self, r: Completion) -> Result<(), ApiError> {
        let (t, owner) = self.task(r.task_id)?;
        let space = self.space(&owner, t.folder_id)?;
        store::insert_completion(self.c, &r)?;
        self.push(Effect::Completion(Link::Added(r)), Side::default(), space.side(), false);
        Ok(())
    }

    fn delete_completion(&mut self, r: Completion, space: &Space) -> Result<(), ApiError> {
        store::delete_completion(self.c, &r)?;
        self.push(Effect::Completion(Link::Removed(r)), space.side(), Side::default(), false);
        Ok(())
    }

    /// Shares folder `f` of `owner`'s with `person`. The first time, its tasks lose their
    /// dependencies on those outside it, as only what is in it is shared.
    fn add_member(&mut self, f: &Folder, owner: &str, person: Person) -> Result<(), ApiError> {
        let space = self.space(owner, Some(f.id))?;
        if space.root.is_some_and(|r| r != f.id) {
            return Err(ApiError::invalid("共享的文件夹里的文件夹不能再共享", None));
        }
        if !space.shared() && !store::shared_within(self.c, f.id)?.is_empty() {
            return Err(ApiError::invalid(format!("“{}”里已有共享的文件夹，不能再共享", f.name.as_str()), None));
        }
        if space.members.contains(&person.id) {
            return Err(ApiError::conflict(format!("{} 已是成员", person.display_name)));
        }
        if !space.shared() {
            if store::shared_name_taken(self.c, owner, f.name.as_str(), f.id)? {
                return Err(ApiError::conflict(format!("已共享过名为“{}”的文件夹，请先改名", f.name.as_str())));
            }
            self.cut_off(f.id, &space)?;
        }
        store::insert_member(self.c, f.id, &person.id)?;
        self.roots.borrow_mut().clear();
        let mut members = space.members.clone();
        members.push(person.id.clone());
        members.sort();
        let before = Share { folder_id: f.id, owner: owner.to_owned(), members: space.members.clone() };
        let after = Share { members, ..before.clone() };
        let side = share_side(&after);
        self.done.resync.push(person.id.clone());
        self.push(Effect::Person(Change::Created(person)), Side::default(), side.clone(), false);
        let change = if space.shared() { Change::Updated { before, after } } else { Change::Created(after) };
        self.push(Effect::Share(change), space.side(), side, false);
        Ok(())
    }

    /// Takes `user` off folder `f`, shared by `owner`: off the tasks they were to do, and their AI
    /// jobs out of it. Once nobody is left, it is no longer shared, and nobody is assigned anything.
    fn remove_member(&mut self, f: &Folder, owner: &str, user: &str) -> Result<(), ApiError> {
        let space = self.space(owner, Some(f.id))?;
        if space.root != Some(f.id) || !space.members.iter().any(|m| m == user) {
            return Err(ApiError::not_found("不是这个文件夹的成员"));
        }
        let last = space.members.len() == 1;
        for t in store::subtree_tasks(self.c, f.id)? {
            let mut after = t.clone();
            after.assignees.retain(|u| !last && u != user);
            if after != t {
                store::update_task(self.c, owner, &after)?;
                self.push(Effect::Task(Change::Updated { before: Box::new(t), after: Box::new(after) }), space.side(), space.side(), false);
            }
        }
        for (j, by) in store::subtree_jobs(self.c, f.id)? {
            if by == user {
                self.drop_job(j, &by)?;
            }
        }
        store::delete_member(self.c, f.id, user)?;
        self.roots.borrow_mut().clear();
        let before = Share { folder_id: f.id, owner: owner.to_owned(), members: space.members.clone() };
        let after = Share { members: before.members.iter().filter(|m| *m != user).cloned().collect(), ..before.clone() };
        self.done.resync.push(user.to_owned());
        let change = if last { Change::Deleted(before) } else { Change::Updated { before, after: after.clone() } };
        self.push(Effect::Share(change), space.side(), share_side(&after), false);
        Ok(())
    }

    /// How often task `id` was done, as each change since changed it, in order, up to this one so far:
    /// `rest`, what came after in the mutation numbered `seq`, then the mutations after it.
    fn completions_since(&self, seq: i64, id: TaskId, rest: &[Effect]) -> Result<Vec<Counted>, ApiError> {
        let mut since = self.c.prepare("SELECT effects FROM mutations WHERE seq > ?1 AND effects LIKE ?2 ORDER BY seq")?;
        let mut effects = rest.to_vec();
        for logged in since.query_map(params![seq, format!("%{id}%")], |r| r.get::<_, String>(0))? {
            effects.extend(serde_json::from_str::<Vec<Effect>>(&logged?)?);
        }
        effects.extend(self.done.seen.iter().map(|s| s.effect.clone()));
        Ok(effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::Task(Change::Updated { before, after }) if after.id == id && before.completions != after.completions => {
                    Some((before.completions, after.completions))
                }
                _ => None,
            })
            .collect())
    }

    /// Restores what a mutation changed, newest change first. Entities that were removed come back
    /// unchanged (same id, and number unless it is taken). Changes whose target is already gone or
    /// back are skipped; bringing something back into a folder or onto a task that is gone fails.
    ///
    /// Only a change the user saw all of can be undone, someone else's in a shared folder too, the
    /// AI jobs in it aside, with their attachments: those are each their owner's, who undoes their
    /// own. An attachment moved from a job to a task is, to everyone else, made there.
    fn undo(&mut self, id: MutationId) -> Result<(), ApiError> {
        let row: Option<(i64, Option<String>, Option<String>)> = self
            .c
            .query_row(
                "SELECT m.seq, m.effects, d.effects FROM mutations m JOIN deliveries d ON d.seq = m.seq AND d.user_id = ?2 WHERE m.id = ?1",
                params![id, self.user],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((seq, Some(all), own)) = row else { return Err(ApiError::not_found("要撤销的改动不存在或已过期")) };
        let all: Vec<Effect> = serde_json::from_str(&all)?;
        let seen: Vec<Effect> = match own {
            Some(own) => serde_json::from_str(&own)?,
            None => all.clone(),
        };
        let owner = |c: &Change<Share>| match c {
            Change::Created(s) | Change::Deleted(s) | Change::Updated { before: s, .. } => s.owner.clone(),
        };
        if all.iter().any(|e| matches!(e, Effect::Share(c) if owner(c) != self.user)) {
            return Err(ApiError::forbidden("只有共享人能添加或移除成员"));
        }
        let of_job = |a: &Attachment| matches!(a.holder, Holder::Job(_));
        let jobs: BTreeSet<AttachmentId> = all
            .iter()
            .filter_map(|e| match e {
                Effect::Attachment(Change::Created(a) | Change::Deleted(a)) if of_job(a) => Some(a.id),
                Effect::Attachment(Change::Updated { before, after }) if of_job(before) || of_job(after) => Some(before.id),
                _ => None,
            })
            .collect();
        let shared = |list: &[Effect]| -> Vec<Effect> {
            list.iter()
                .filter(|e| match e {
                    Effect::AiJob(_) | Effect::Person(_) => false,
                    Effect::Attachment(Change::Created(a) | Change::Deleted(a) | Change::Updated { before: a, .. }) => {
                        !jobs.contains(&a.id)
                    }
                    _ => true,
                })
                .cloned()
                .collect()
        };
        if shared(&all) != shared(&seen) {
            return Err(ApiError::forbidden("这个批次里有无权查看的改动，无法撤销"));
        }
        // For the tasks carried back with a folder, the numbers they had: while undoing this only.
        for e in &seen {
            if let Effect::Task(Change::Updated { before, .. }) = e {
                self.numbers.entry(before.id).or_insert(before.number);
            }
        }
        // A folder it made goes empty, in turn, freeing its name for one it deleted. Holding what was put
        // in it since, it waits for the end: what it made goes in turn before, what else stays, and the
        // folder with that.
        let ours: HashSet<FolderId> = seen
            .iter()
            .filter_map(|e| match e {
                Effect::Folder(Change::Created(f)) => Some(f.id),
                _ => None,
            })
            .collect();
        let mut made = Vec::new();
        // The tasks this brings back, or sets back to how often they were done: only their records come
        // back, one back from elsewhere having those it came back with. A deletion took the records out
        // before the task, so going backwards the task comes back first; a count is known up front, as
        // a time done again before a time kept a record each had its earlier record taken out after
        // the count went up.
        let mut counted: HashSet<TaskId> = seen
            .iter()
            .filter_map(|e| match e {
                Effect::Task(Change::Updated { before, after }) if before.completions != after.completions => Some(before.id),
                _ => None,
            })
            .collect();
        for (i, effect) in seen.iter().cloned().enumerate().rev() {
            match effect {
                Effect::Folder(Change::Created(f)) => match self.visible_folder(f.id)? {
                    Some((_, owner)) if only_folders_of(self.c, f.id, &HashSet::from([f.id]))? => self.delete_folder(f.id, &owner)?,
                    Some(_) => made.push(f.id),
                    None => {}
                },
                Effect::Folder(Change::Deleted(f)) => {
                    // Its ID taken again by one it made, which still holds what was put in it since.
                    if made.contains(&f.id) {
                        return Err(ApiError::conflict(format!(
                            "文件夹“{}”无法恢复：之后又往取代它的文件夹里放了东西，请先移走",
                            f.name.as_str()
                        )));
                    }
                    if store::load_folder(self.c, f.id)?.is_none() {
                        let owner = self
                            .owner_in(f.parent_id)
                            .map_err(|_| ApiError::not_found(format!("文件夹“{}”原所在的文件夹已删除", f.name.as_str())))?;
                        self.create_folder(Folder { updated_at: self.now, ..f }, &owner)?;
                    }
                }
                Effect::Folder(Change::Updated { before: b, after: a }) => {
                    if let Some((cur, owner)) = self.visible_folder(b.id)? {
                        let restored = restore!(cur, b, a, parent_id, name, sort_key, color, icon);
                        self.update_folder(cur, &owner, restored)?;
                    }
                }
                Effect::Task(Change::Created(t)) => {
                    if self.visible_task(t.id)?.is_some() {
                        self.delete_task(t.id)?;
                    }
                }
                Effect::Task(Change::Deleted(t)) => {
                    if store::load_task(self.c, t.id)?.is_none() {
                        let owner = self
                            .owner_in(t.folder_id)
                            .map_err(|_| ApiError::not_found(format!("“{}”原所在的文件夹已删除", t.title.as_str())))?;
                        let number = self.number(&owner, t.id, Some(t.number))?;
                        let restored =
                            Task { number, updated_at: self.now, updated_by: self.user.to_owned(), updated_via: self.via.clone(), ..*t };
                        counted.insert(t.id);
                        self.create_task(restored, &owner)?;
                    }
                }
                Effect::Task(Change::Updated { before: b, after: a }) => {
                    if let Some((cur, owner)) = self.visible_task(b.id)? {
                        // Done again or taken back since, maybe to the same count on the same day: setting
                        // the count back would undo that too.
                        if b.completions != a.completions
                            && (cur.completions != a.completions
                                || !still_last(&b.completions, &a.completions, self.completions_since(seq, b.id, &seen[i + 1..])?))
                        {
                            return Err(ApiError::conflict(format!("“{}”在这之后又完成或撤销过，请先撤销后来的那次", cur.title.as_str())));
                        }
                        let mut restored =
                            restore!(cur, b, a, folder_id, title, description, done_at, start, start_time, completions, sort_key);
                        // A time without its date, which was cleared since, goes too.
                        restored.start_time = restored.start_time.filter(|_| restored.start.is_some());
                        restored.due = restore_due(&cur.due, &b.due, &a.due);
                        restored.assignees = restore_people(&cur.assignees, &b.assignees, &a.assignees);
                        self.update_task(cur, &owner, restored, Some(b.number))?;
                    }
                }
                Effect::Dependency(Link::Added(d)) => {
                    if let Some(d) = store::load_dependency(self.c, d.before, d.after)?
                        && let Some((t, owner)) = self.visible_task(d.before)?
                    {
                        let space = self.space(&owner, t.folder_id)?;
                        self.delete_dependency(d, &space)?;
                    }
                }
                Effect::Dependency(Link::Removed(d)) => {
                    let (Some((b, b_owner)), Some((a, a_owner))) = (self.visible_task(d.before)?, self.visible_task(d.after)?) else {
                        return Err(ApiError::not_found("要恢复的依赖所关联的事项已删除"));
                    };
                    let space = self.space(&b_owner, b.folder_id)?;
                    // Moved apart since, they can't be linked again.
                    if store::load_dependency(self.c, d.before, d.after)?.is_none() && space == self.space(&a_owner, a.folder_id)? {
                        self.create_dependency(d, &space)?;
                    }
                }
                Effect::Completion(Link::Added(r)) => {
                    if let Some(r) = store::load_completion(self.c, r.task_id, &r.due, r.again)?
                        && let Some((t, owner)) = self.visible_task(r.task_id)?
                    {
                        let space = self.space(&owner, t.folder_id)?;
                        self.delete_completion(r, &space)?;
                    }
                }
                Effect::Completion(Link::Removed(r)) => {
                    if counted.contains(&r.task_id)
                        && self.visible_task(r.task_id)?.is_some()
                        && store::load_completion(self.c, r.task_id, &r.due, r.again)?.is_none()
                    {
                        self.record_completion(r)?;
                    }
                }
                Effect::Attachment(Change::Created(a)) => {
                    if let Some((cur, side)) = self.visible_attachment(a.id)? {
                        self.delete_attachment(cur, side)?;
                    }
                }
                Effect::Attachment(Change::Deleted(a)) => {
                    if store::load_attachment(self.c, a.id)?.is_none() {
                        // Whoever uploaded it: what it belongs to says who sees it.
                        if store::load_file(self.c, a.id)?.is_none() {
                            return Err(ApiError::not_found(format!("附件“{}”的文件已不在服务器上", a.name.as_str())));
                        }
                        let side = self
                            .holder_side(a.holder)
                            .map_err(|_| ApiError::not_found(format!("附件“{}”所属的事项已删除", a.name.as_str())))?;
                        self.create_attachment(a, side)?;
                    }
                }
                Effect::Attachment(Change::Updated { before: b, after: a }) => {
                    if let Some((cur, side)) = self.visible_attachment(b.id)? {
                        let restored = restore!(cur, b, a, name, holder);
                        self.update_attachment(cur, side, restored)?;
                    }
                }
                Effect::AiJob(Change::Created(j)) => {
                    if let Some(cur) = store::load_job(self.c, self.user, j.id)? {
                        let user = self.user;
                        self.delete_job(cur, user)?;
                    }
                }
                Effect::AiJob(Change::Deleted(j)) => {
                    if store::load_job(self.c, self.user, j.id)?.is_none() {
                        if let Some(f) = j.folder_id
                            && self.visible_folder(f)?.is_none()
                        {
                            return Err(ApiError::not_found("AI 整理请求原所在的文件夹已删除"));
                        }
                        // What it was being organized into was thrown away, or is undone now.
                        let j = if j.state == JobState::Running {
                            AiJob { state: JobState::Failed, error: Some("已撤销".into()), ..*j }
                        } else {
                            *j
                        };
                        self.create_job(AiJob { updated_at: self.now, ..j })?;
                    }
                }
                // Starting, failing and retrying are the job's own course, not undone; nor are names.
                Effect::AiJob(Change::Updated { .. }) | Effect::Person(_) => {}
                Effect::Share(change) => {
                    let (folder, b, a) = match change {
                        Change::Created(a) => (a.folder_id, Vec::new(), a.members),
                        Change::Updated { before, after } => (before.folder_id, before.members, after.members),
                        Change::Deleted(b) => (b.folder_id, b.members, Vec::new()),
                    };
                    let Some((f, owner)) = self.visible_folder(folder)? else { continue };
                    // Those it added go, and those it took off come back, as far as nothing has done so since.
                    let now = store::members(self.c, folder)?;
                    for user in a.iter().filter(|u| !b.contains(u) && now.contains(u)) {
                        self.remove_member(&f, &owner, user)?;
                    }
                    for user in b.iter().filter(|u| !a.contains(u) && !now.contains(u)) {
                        let person = store::person(self.c, user)?;
                        self.add_member(&f, &owner, person)?;
                    }
                }
            }
        }
        for id in made {
            if let Some((_, owner)) = self.visible_folder(id)?
                && only_folders_of(self.c, id, &ours)?
            {
                self.delete_folder(id, &owner)?;
            }
        }
        self.numbers.clear();
        Ok(())
    }

    fn visible_folder(&self, id: FolderId) -> Result<Option<(Folder, String)>, ApiError> {
        match self.folder(id) {
            Ok(found) => Ok(Some(found)),
            Err(e) if e.code() == "not_found" => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn visible_attachment(&self, id: AttachmentId) -> Result<Option<(Attachment, Side)>, ApiError> {
        let Some(a) = store::load_attachment(self.c, id)? else { return Ok(None) };
        match self.holder_side(a.holder) {
            Ok(side) => Ok(Some((a, side))),
            Err(e) if e.code() == "not_found" => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn visible_task(&self, id: TaskId) -> Result<Option<(Task, String)>, ApiError> {
        match self.task(id) {
            Ok(found) => Ok(Some(found)),
            Err(e) if e.code() == "not_found" => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Who sees a share: its owner and members.
fn share_side(s: &Share) -> Side {
    Side::of(std::iter::once(&s.owner).chain(&s.members).cloned().collect())
}

/// Whether nothing is in a folder but folders in `only`, and in them the same: no task, no AI job.
fn only_folders_of(c: &Connection, id: FolderId, only: &HashSet<FolderId>) -> rusqlite::Result<bool> {
    Ok(store::subtree_tasks(c, id)?.is_empty()
        && store::subtree_jobs(c, id)?.is_empty()
        && store::subtree_folders(c, id)?.iter().all(|f| only.contains(&f.id)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_time_done_is_still_the_last_as_the_fixtures_say() {
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            done: (i64, i64),
            later: Vec<(i64, i64)>,
            last: bool,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/undo-done.json")).unwrap();
        for c in cases {
            assert_eq!(super::still_last(&c.done.0, &c.done.1, c.later), c.last, "{}", c.name);
        }
    }
}
