//! Writing a change read from the drive (docs/local.md#读入): what it did, as it was done on the
//! device that made it, without doing its operations again. That device worked on the same data, so
//! every device ends up with the same, whatever version of the app each runs.

use nexttodo_core::model::{AiJob, Attachment, Change, Effect, JobState, Link, MutationId};
use nexttodo_core::ops::Op;
use rusqlite::{Connection, params};

use super::commit::Cleared;
use crate::error::ApiError;
use crate::mutate::{self, Actor, Done, Recorded};
use crate::store;

/// Marks files `cleared`, with the records of those this device has none of: uploaded on another
/// device only.
pub(crate) fn purge(c: &Connection, user: &str, cleared: &[Cleared]) -> rusqlite::Result<()> {
    for f in cleared {
        c.execute(
            "INSERT INTO files (id, owner_id, type, size, created_at, unused_at, purged) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
             ON CONFLICT (id) DO UPDATE SET type = ?3, size = ?4, created_at = ?5, unused_at = ?6, purged = 1, staged = 0",
            params![f.id, user, f.media_type, f.size, f.created_at, f.unused_at],
        )?;
    }
    Ok(())
}

/// Writes what mutation `id`, made at `at`, did, and logs it as made by `actor`.
pub(crate) fn mutation(
    c: &Connection,
    actor: &Actor,
    id: MutationId,
    at: i64,
    ops: &[Op],
    effects: Vec<Effect>,
) -> Result<Recorded, ApiError> {
    for effect in &effects {
        write(c, actor.user, id, at, effect)?;
    }
    mutate::record(c, actor, id, ops, &Done::replayed(effects, actor.user), at)
}

fn write(c: &Connection, user: &str, id: MutationId, at: i64, effect: &Effect) -> Result<(), ApiError> {
    match effect {
        Effect::Folder(Change::Created(f)) => store::insert_folder(c, user, f)?,
        Effect::Folder(Change::Updated { after, .. }) => store::update_folder(c, after)?,
        Effect::Folder(Change::Deleted(f)) => store::delete_folder(c, f.id)?,
        Effect::Task(Change::Created(t)) => {
            store::insert_task(c, user, t)?;
            numbered(c, user, t.number)?;
        }
        Effect::Task(Change::Updated { after, .. }) => {
            store::update_task(c, user, after)?;
            numbered(c, user, after.number)?;
        }
        Effect::Task(Change::Deleted(t)) => store::delete_task(c, t.id)?,
        Effect::Dependency(Link::Added(d)) => store::insert_dependency(c, user, d)?,
        Effect::Dependency(Link::Removed(d)) => store::delete_dependency(c, d)?,
        Effect::Completion(Link::Added(r)) => store::insert_completion(c, r)?,
        Effect::Completion(Link::Removed(r)) => store::delete_completion(c, r)?,
        Effect::Attachment(Change::Created(a)) => {
            shared_file(c, user, a, at)?;
            store::insert_attachment(c, a)?;
        }
        Effect::Attachment(Change::Updated { after, .. }) => store::update_attachment(c, after)?,
        Effect::Attachment(Change::Deleted(a)) => store::delete_attachment(c, a.id, at)?,
        Effect::AiJob(Change::Created(j)) => store::insert_job(c, user, j)?,
        Effect::AiJob(Change::Updated { before, after }) => {
            store::update_job(c, after)?;
            round(c, id, before, after)?;
        }
        Effect::AiJob(Change::Deleted(j)) => store::delete_job(c, j.id)?,
        // Local mode has one user, whose name never changes.
        Effect::Person(_) => {}
        Effect::Share(_) => return Err(ApiError::invalid("网盘里的改动不能包含共享", None)),
    }
    Ok(())
}

/// The next number is past `number`, as it was where the task got it.
fn numbered(c: &Connection, user: &str, number: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE users SET next_task_number = max(next_task_number, ?2 + 1) WHERE id = ?1", params![user, number])?;
    Ok(())
}

/// The record of the file an attachment is made of, as the attachment says: every device has the same,
/// whether it uploaded the file or not. One uploaded here is no longer this device's alone.
fn shared_file(c: &Connection, user: &str, a: &Attachment, at: i64) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO files (id, owner_id, type, size, created_at, unused_at, staged) VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0)
         ON CONFLICT (id) DO UPDATE SET type = ?3, size = ?4, created_at = iif(staged, ?5, created_at), staged = 0",
        params![a.id, user, a.media_type, a.size, at],
    )?;
    Ok(())
}

/// A job started is in the round mutation `id` began; one no longer running is in none.
fn round(c: &Connection, id: MutationId, before: &AiJob, after: &AiJob) -> rusqlite::Result<()> {
    match (before.state, after.state) {
        (JobState::Running, JobState::Running) => {}
        (_, JobState::Running) => {
            c.execute("UPDATE ai_jobs SET run = ?2 WHERE id = ?1", params![after.id, id])?;
        }
        _ => {
            c.execute("UPDATE ai_jobs SET run = NULL WHERE id = ?1", [after.id])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use nexttodo_core::model::{FileSize, JobState, MediaType};
    use nexttodo_core::ops::Mutation;
    use serde_json::json;

    use super::*;
    use crate::Db;
    use crate::drive::tests::{SHARED, dump, id, same_user};
    use crate::store::StoredFile;

    /// Two devices with the same local user: `a` does the changes, `b` writes what they did.
    struct Pair {
        a: Db,
        b: Db,
        user: String,
        /// How far into `a`'s log `b` has read.
        read: i64,
        now: i64,
    }

    impl Pair {
        fn new() -> Pair {
            let (mut dbs, user) = same_user(2);
            let (b, a) = (dbs.pop().unwrap(), dbs.pop().unwrap());
            Pair { a, b, user, read: 0, now: 1_800_000_000_000 }
        }

        /// Makes the change on `a` and returns its id.
        fn change(&mut self, ops: serde_json::Value) -> String {
            let m: Mutation = serde_json::from_value(json!({ "id": id(), "ops": ops })).unwrap();
            self.now += 1000;
            let now = self.now;
            self.a.hold(|c| mutate::apply(c, &Actor { user: &self.user, token_id: "", via: None }, &m, now).unwrap());
            m.id.0.to_string()
        }

        /// Writes on `b` what `a` did since last time.
        fn sync(&mut self) {
            let read = self.read;
            let log: Vec<(i64, MutationId, i64, String, String, Option<String>)> = self.a.hold(|c| {
                c.prepare("SELECT seq, id, created_at, ops, effects, actor FROM mutations WHERE seq > ?1 ORDER BY seq")
                    .unwrap()
                    .query_map([read], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap()
            });
            for (seq, id, at, ops, effects, via) in log {
                let (ops, effects): (Vec<Op>, Vec<Effect>) = (serde_json::from_str(&ops).unwrap(), serde_json::from_str(&effects).unwrap());
                self.b.hold(|c| {
                    let tx = c.transaction().unwrap();
                    mutation(&tx, &Actor { user: &self.user, token_id: "", via: via.as_deref() }, id, at, &ops, effects).unwrap();
                    tx.commit().unwrap();
                });
                self.read = seq;
            }
        }

        /// Everything the two share is the same, but the rounds of AI jobs: `a` ran its job
        /// without the drive.
        fn assert_same(&self) {
            for sql in SHARED {
                let sql = sql.replace("SELECT * FROM ai_jobs", "SELECT id, owner_id, folder_id, kind, text, local_time, state, error, suggestions, created_at, updated_at, reminders FROM ai_jobs");
                assert_eq!(dump(&self.a, &sql), dump(&self.b, &sql), "{sql}");
            }
        }
    }

    #[test]
    fn writing_what_a_change_did_gives_the_same_data_as_making_it() {
        let mut p = Pair::new();
        let (top, inner, t1, t2, t3, t4) = (id(), id(), id(), id(), id(), id());
        p.change(json!([
            { "op": "folder.create", "id": top, "name": "论文" },
            { "op": "folder.create", "id": inner, "name": "实验", "parent_id": top, "color": "blue", "icon": "flask" },
            { "op": "task.create", "id": t1, "title": "设计问卷", "folder_id": top, "due": { "date": "2026-11-20", "time": "18:00", "remind_before": 60 } },
            { "op": "task.create", "id": t2, "title": "发问卷", "folder_id": inner },
            { "op": "task.create", "id": t3, "title": "每周组会", "due": { "date": "2026-11-18", "repeat": { "every": 1, "unit": "week", "from": "due" } } },
            { "op": "task.create", "id": t4, "title": "给家里打电话", "due": { "date": "2026-09-25", "repeat": { "every": 12, "unit": "month", "from": "due", "days": [15], "lunar": true } } },
            { "op": "dep.add", "before": t1, "after": t2 },
        ]));
        p.change(json!([{ "op": "task.update", "id": t1, "title": "设计并预测问卷", "done": true }]));
        p.change(json!([{ "op": "task.update", "id": t3, "done": true, "today": "2026-11-18" }]));
        p.change(json!([{ "op": "task.update", "id": t4, "done": true, "today": "2026-09-25" }]));
        p.change(json!([{ "op": "task.update", "id": t2, "folder_id": null }]));
        p.change(json!([{ "op": "folder.update", "id": top, "color": "green", "icon": "graduation-cap" }]));

        // An attachment, its file uploaded first.
        let file = id();
        let (stored, now) = (
            StoredFile {
                owner: p.user.clone(),
                media_type: MediaType::try_from("image/png".to_owned()).unwrap(),
                size: FileSize::try_from(10).unwrap(),
            },
            p.now,
        );
        p.a.hold(|c| store::insert_file(c, serde_json::from_value(json!(file)).unwrap(), &stored, now, false).unwrap());
        p.change(json!([{ "op": "attachment.add", "id": file, "name": "图 1.png", "type": "image/png", "size": 10, "task_id": t1 }]));

        // An AI job started by the server, as the queue does.
        let job = id();
        p.change(json!([{
            "op": "ai_job.create", "id": job, "text": "下周要交开题报告", "local_time": "2026-11-17 16:30 星期二",
            "reminders": { "timed": [{ "before": 30 }], "dated": [] },
        }]));
        let started = p.a.hold(|c| {
            mutate::set_job_state(
                c,
                &Actor { user: &p.user, token_id: "", via: Some("AI 整理") },
                serde_json::from_value(json!(job)).unwrap(),
                JobState::Waiting,
                JobState::Running,
                None,
                now + 1,
            )
            .unwrap()
        });
        assert!(started.is_some());

        p.sync();
        p.assert_same();
        let run: Option<String> = p.b.hold(|c| c.query_row("SELECT run FROM ai_jobs WHERE id = ?1", [&job], |r| r.get(0)).unwrap());
        assert!(run.is_some(), "a job started is in a round");

        // Deleting the folder takes everything in it; undoing brings it back.
        let deleted = p.change(json!([{ "op": "folder.delete", "id": top }]));
        p.change(json!([{ "op": "task.delete", "id": t3 }]));
        p.sync();
        p.assert_same();
        p.change(json!([{ "op": "undo", "mutation": deleted }]));
        p.change(json!([{ "op": "ai_job.delete", "id": job }]));
        p.sync();
        p.assert_same();
        let numbered: i64 = p.b.hold(|c| c.query_row("SELECT next_task_number FROM users", [], |r| r.get(0)).unwrap());
        assert_eq!(numbered, 5);
    }
}
