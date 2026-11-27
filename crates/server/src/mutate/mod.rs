//! The mutation pipeline: validate and apply operations in one transaction, record the resulting
//! entity changes in the mutation log. Resubmitting a mutation id returns the first result.
//!
//! What a user can see and change is their own and what is in the folders shared with them
//! (docs/design/sharing.md). Everything in a shared folder is its owner's, so the owner of what is made
//! somewhere is the owner of where it is: the folder's, or at the root the user's. Each part of a
//! change goes to whoever sees it, before or after; someone who sees only one side of it sees
//! something made or deleted.

mod check;
mod ops;
mod undo;
mod writes;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use nexttodo_core::board::{Now, comes_round, holds_up};
use nexttodo_core::model::{
    AiJob, AiJobId, Attachment, Change, Completion, Dependency, Effect, Folder, FolderId, JobState, Link, MutationId, Name, NoteText,
    Person, Task, TaskId,
};
use nexttodo_core::ops::{MAX_OPS, Mutation, Op};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::error::ApiError;
use crate::store;
use crate::tools::write::copy_name;

pub(crate) use check::{at_times, both_due, check_completion_date};

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
    let done = run(&tx, actor, &m.ops, now, &local(m, now))?;
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

/// The day and time on the device that made `m` (docs/api.md#改动): as given with it, or with a task
/// done in it, or else UTC's.
pub fn local(m: &Mutation, now: i64) -> Now {
    let done_on = m.ops.iter().find_map(|op| match op {
        Op::TaskUpdate { today: Some(today), .. } => Some(today),
        _ => None,
    });
    match (&m.today, done_on) {
        (Some(today), _) => Now { today: today.clone(), time: m.time.clone() },
        (None, Some(today)) => Now { today: today.clone(), time: None },
        (None, None) => Now::utc(now),
    }
}

/// Applies the operations inside the caller's transaction, made on a device at `local`, and returns
/// what changed. At most `MAX_OPS` at once, however they were made: one change is one transaction on
/// the one connection.
pub fn run(c: &Connection, actor: &Actor, ops: &[Op], now: i64, local: &Now) -> Result<Done, ApiError> {
    if ops.len() > MAX_OPS {
        return Err(ApiError::invalid(format!("每次最多 {MAX_OPS} 个操作，本次有 {} 个", ops.len()), None));
    }
    let mut cx = Cx::new(c, actor, now);
    for op in ops {
        cx.op(op, local)?;
    }
    cx.settle(local)?;
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

/// What an import adds to a user's data (docs/local/backup.md#导入): all of it new here, with ids of its own,
/// each folder after the one it is in, the tasks in the order to number them, the dependencies in no
/// circle.
pub struct Import {
    pub folders: Vec<Folder>,
    pub tasks: Vec<Task>,
    pub dependencies: Vec<Dependency>,
    pub completions: Vec<Completion>,
    pub jobs: Vec<AiJob>,
    pub attachments: Vec<Attachment>,
    pub notes: Vec<(Option<FolderId>, NoteText)>,
}

/// Adds `import` to the actor's data as change `id`, made and undone as one: the tasks numbered after
/// theirs, a folder at the root whose name is taken called as a copy would be, each attachment taking
/// the file uploaded for it, as the backup has it, or none is. Made already, it is what it did then. The notes are not part of the
/// change, as they never are; a global one already there stays.
pub fn import(conn: &mut Connection, actor: &Actor, id: MutationId, import: Import, now: i64) -> Result<(Applied, Vec<String>), ApiError> {
    let tx = conn.transaction()?;
    if let Some(applied) = earlier(&tx, actor, id)? {
        return Ok((applied, Vec::new()));
    }
    let user = actor.user;
    let mut cx = Cx::new(&tx, actor, now);
    let mut roots = store::root_folder_names(&tx, user)?;
    for mut f in import.folders {
        if f.parent_id.is_none() {
            f.name = Name::try_from(copy_name(f.name.as_str(), |n| roots.contains(n))).expect("cut short to fit");
            roots.insert(f.name.as_str().to_owned());
        }
        cx.create_folder(f, user)?;
    }
    for mut t in import.tasks {
        t.number = store::take_task_number(&tx, user)?;
        (t.created_by, t.updated_by) = (user.to_owned(), user.to_owned());
        (t.created_via, t.updated_via) = (cx.via.clone(), cx.via.clone());
        cx.create_task(t, user)?;
    }
    // Between tasks new here, which no one else sees: none closes a circle with those already here, and
    // the caller found none among them.
    let side = cx.space(user, None)?.side();
    for d in import.dependencies {
        store::insert_dependency(&tx, user, &d)?;
        cx.push(Effect::Dependency(Link::Added(d)), Side::default(), side.clone(), false);
    }
    for r in import.completions {
        cx.record_completion(r)?;
    }
    for j in import.jobs {
        cx.create_job(j)?;
    }
    let mut not_uploaded = Vec::new();
    for a in &import.attachments {
        match store::load_file(&tx, a.id)? {
            Some(f) if f.owner == user && f.size == a.size && !store::attachment_id_taken(&tx, a.id)? => {
                // Uploaded as bytes alone: what they are is what the backup says.
                store::set_file_type(&tx, a.id, &a.media_type)?;
                let side = cx.holder_side(a.holder)?;
                cx.create_attachment(a.clone(), side)?;
            }
            _ => not_uploaded.push(a.name.as_str()),
        }
    }
    if !not_uploaded.is_empty() {
        return Err(ApiError::not_uploaded(&not_uploaded));
    }
    let had = store::ai_notes(&tx, user)?;
    for (folder, text) in import.notes.into_iter().filter(|(folder, _)| folder.is_some() || !had.contains_key(&None)) {
        store::set_ai_note(&tx, user, folder, &text, now)?;
    }
    let done = cx.done;
    let recorded = record(&tx, actor, id, &[], &done, now)?;
    tx.commit()?;
    Ok((Applied { seq: recorded.seq, effects: done.view(user) }, recorded.to))
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
    /// who sees all of it (docs/local/drive.md#读入).
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
                    Effect::Prefs(u) => (b && a && !s.kept).then(|| Effect::Prefs(u.clone())),
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

    /// Of the tasks due some days after their prerequisites are done that this change concerns, gives
    /// those with all of them done the day it was made, `local`'s, and takes that back from those a
    /// prerequisite came to hold up in it (docs/design/dates.md#前置完成后的截止). Only on such changes, and
    /// not for a repeating prerequisite done before: its time come round again, by the clock or by an
    /// undo, it leaves its tasks be.
    fn settle(&mut self, local: &Now) -> Result<(), ApiError> {
        let mut concerned = BTreeSet::new();
        // A prerequisite that came to hold up a task, and the task.
        let mut held = HashSet::new();
        // Tasks that came to hold up those after them, or stopped.
        let mut turned = Vec::new();
        // Not what only came into or out of someone's view with a folder moved.
        for s in self.done.seen.iter().filter(|s| !s.kept) {
            match &s.effect {
                Effect::Task(Change::Created(t)) if t.due_after.is_some() => _ = concerned.insert(t.id),
                Effect::Task(Change::Updated { before, after }) => {
                    if after.due_after.is_some() {
                        concerned.insert(after.id);
                    }
                    let holds = holds_up(after, local);
                    if holds != holds_up(before, local) {
                        turned.push((after.id, holds));
                    }
                }
                Effect::Dependency(Link::Added(d)) => {
                    concerned.insert(d.after);
                    held.insert((d.before, d.after));
                }
                Effect::Dependency(Link::Removed(d)) => _ = concerned.insert(d.after),
                _ => {}
            }
        }
        for (id, holds) in turned {
            for d in store::task_dependencies(self.c, id)?.into_iter().filter(|d| d.before == id) {
                concerned.insert(d.after);
                if holds {
                    held.insert((d.before, d.after));
                }
            }
        }
        for id in concerned {
            let Some((t, owner)) = store::load_task(self.c, id)?.filter(|(t, _)| t.due_after.is_some()) else { continue };
            let mut holding = Vec::new();
            for d in store::task_dependencies(self.c, id)?.into_iter().filter(|d| d.after == id) {
                let (p, _) = store::load_task(self.c, d.before)?.expect("a dependency's tasks are there");
                if holds_up(&p, local) {
                    holding.push(p);
                }
            }
            let unlocked_on = match &t.unlocked_on {
                None if holding.is_empty() => Some(local.today.clone()),
                Some(_) if holding.iter().any(|p| held.contains(&(p.id, id)) && !comes_round(p)) => None,
                _ => continue,
            };
            self.update_task(t.clone(), &owner, Task { unlocked_on, ..t }, None)?;
        }
        Ok(())
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
}
