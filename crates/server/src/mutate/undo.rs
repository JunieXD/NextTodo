//! Undoing a change: setting back what it changed, part by part, and leaving what changed since.

use std::collections::{BTreeSet, HashSet};

use nexttodo_core::model::{
    AiJob, Attachment, AttachmentId, Change, Completions, Due, Effect, Folder, FolderId, Holder, JobState, Link, MutationId, Share, Task,
    TaskId,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::ApiError;
use crate::store;

use super::Cx;

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
/// it that still counted, or counted itself (docs/design/history.md).
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

macro_rules! restore {
    ($cur:expr, $before:expr, $after:expr, $($field:ident),*) => {{
        let mut restored = $cur.clone();
        $(if $before.$field != $after.$field {
            restored.$field = $before.$field.clone();
        })*
        restored
    }};
}

impl Cx<'_> {
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
    pub(super) fn undo(&mut self, id: MutationId) -> Result<(), ApiError> {
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
                    Effect::AiJob(_) | Effect::Person(_) | Effect::Prefs(_) => false,
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
        let edited: HashSet<_> = seen.windows(2).filter_map(|e| Effect::completion_edit(&e[0], &e[1]).map(|(r, _)| r.task_id)).collect();
        let mut skip = None;
        for (i, effect) in seen.iter().cloned().enumerate().rev() {
            if skip == Some(i) {
                continue;
            }
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
                        // A date correction may reschedule a repeat even when its summary stays the same.
                        let moved = edited.contains(&b.id) && (b.due != a.due || b.start != a.start || b.done_at != a.done_at);
                        if (moved || b.completions != a.completions)
                            && (cur.completions != a.completions
                                || !still_last(&b.completions, &a.completions, self.completions_since(seq, b.id, &seen[i + 1..])?))
                        {
                            return Err(ApiError::conflict(format!("“{}”在这之后又完成或撤销过，请先撤销后来的那次", cur.title.as_str())));
                        }
                        let mut restored = restore!(
                            cur,
                            b,
                            a,
                            folder_id,
                            title,
                            description,
                            done_at,
                            dropped,
                            start,
                            start_time,
                            start_reminders,
                            remind_at,
                            due_after,
                            unlocked_on,
                            completions,
                            sort_key
                        );
                        if b.done_at.is_some() && a.done_at.is_some() && b.dropped == a.dropped && cur.done_at != a.done_at {
                            restored.done_at = cur.done_at;
                        }
                        // A time without its date, which was cleared since, goes too; so does the day
                        // prerequisites were done, without a due date counting from it, and giving up
                        // one open since.
                        restored.start_time = restored.start_time.filter(|_| restored.start.is_some());
                        restored.fit_start_reminders();
                        restored.unlocked_on = restored.unlocked_on.filter(|_| restored.due_after.is_some());
                        restored.dropped &= restored.done_at.is_some();
                        // A due date `due_after` gave is given again.
                        let set = |t: &Task| t.due.clone().filter(|_| t.due_after.is_none());
                        restored.due = restore_due(&set(&cur), &set(&b), &set(&a));
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
                    let replaced = i.checked_sub(1).and_then(|prev| {
                        let (old, _) = Effect::completion_edit(&seen[prev], &seen[i])?;
                        skip = Some(prev);
                        Some(old)
                    });
                    if let Some(cur) = store::load_completion(self.c, r.task_id, &r.due, r.again)?
                        && let Some((t, owner)) = self.visible_task(r.task_id)?
                    {
                        let space = self.space(&owner, t.folder_id)?;
                        if replaced.is_some() && cur != r {
                            return Err(ApiError::conflict("该完成记录后来又修改过，请先撤销后来的改动"));
                        }
                        self.delete_completion(cur, &space)?;
                        if let Some(old) = replaced {
                            self.record_completion(old.clone())?;
                        }
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
                // Starting, failing and retrying are the job's own course, not undone; nor are names and
                // settings (docs/ui/settings.md#跟着账号的设置).
                Effect::AiJob(Change::Updated { .. }) | Effect::Person(_) | Effect::Prefs(_) => {}
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

    fn visible_task(&self, id: TaskId) -> Result<Option<(Task, String)>, ApiError> {
        match self.task(id) {
            Ok(found) => Ok(Some(found)),
            Err(e) if e.code() == "not_found" => Ok(None),
            Err(e) => Err(e),
        }
    }
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
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../../../fixtures/undo-done.json")).unwrap();
        for c in cases {
            assert_eq!(super::still_last(&c.done.0, &c.done.1, c.later), c.last, "{}", c.name);
        }
    }
}
