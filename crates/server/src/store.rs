//! Reading and writing entities. Lookups by id find whatever has it; who can see it is for the caller
//! to decide, by its owner and the folder it is in (docs/design.md#共享). Lists are those of a user.

use std::collections::HashMap;

use nexttodo_core::board::Snapshot;
use nexttodo_core::model::{
    AiJob, AiJobId, Attachment, AttachmentId, Completion, Completions, Date, Dependency, Due, Every, FileSize, Folder, FolderId, Holder,
    MediaType, Monthdays, NoteText, Period, Person, Remind, Repeat, RepeatFrom, RepeatUnit, Share, SortKey, Task, TaskId, Time, Weekdays,
};
use rusqlite::{Connection, OptionalExtension, Params, Row, params};

const FOLDER_COLUMNS: &str = "id, parent_id, name, sort_key, created_at, updated_at, color, icon";
const JOB_COLUMNS: &str = "id, folder_id, kind, text, local_time, state, error, suggestions, created_at, updated_at, reminders";
/// Attachments, with what their files are.
const ATTACHMENTS: &str =
    "SELECT a.id, a.task_id, a.job_id, a.name, f.type, f.size, a.created_at FROM attachments a JOIN files f ON f.id = a.id";
macro_rules! task_columns {
    () => {
        "id, number, folder_id, title, description, done_at, due_date, due_time, reminders, sort_key, created_at, updated_at, created_via, updated_via, repeat_every, repeat_unit, repeat_from, repeat_days, completion_count, last_completed, repeat_monthdays, start_date, created_by, updated_by, start_time, repeat_until, repeat_lunar, due_after, unlocked_on"
    };
}
const TASK_COLUMNS: &str = task_columns!();
/// A task's columns and its assignees, from `tasks`.
const TASK_SELECT: &str = concat!(
    task_columns!(),
    ", (SELECT json_group_array(user_id) FROM (SELECT user_id FROM assignees WHERE task_id = tasks.id ORDER BY user_id))"
);

/// Folder ids in the subtree rooted at `?1`, including the root.
const SUBTREE: &str = "WITH RECURSIVE sub(id, depth) AS (
    SELECT ?1, 0 UNION ALL SELECT f.id, sub.depth + 1 FROM folders f JOIN sub ON f.parent_id = sub.id)";

/// The folders shared with user `?1` and everything in them.
const SHARED: &str = "WITH RECURSIVE shared(id) AS (
    SELECT folder_id FROM members WHERE user_id = ?1 UNION ALL SELECT f.id FROM folders f JOIN shared ON f.parent_id = shared.id)";

fn folder_row(r: &Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        name: r.get(2)?,
        sort_key: r.get(3)?,
        color: r.get(6)?,
        icon: r.get(7)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

fn task_row(r: &Row) -> rusqlite::Result<Task> {
    let date: Option<Date> = r.get(6)?;
    let time: Option<Time> = r.get(7)?;
    let reminders: Option<String> = r.get(8)?;
    let reminders: Vec<Remind> = match reminders {
        Some(json) => serde_json::from_str(&json)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e)))?,
        None => Vec::new(),
    };
    // Set together or not at all, as the table checks.
    let repeat = match (r.get(14)?, r.get(15)?, r.get(16)?) {
        (Some(every), Some(unit), Some(from)) => {
            let period = match unit {
                RepeatUnit::Day => Period::Day,
                RepeatUnit::Week => Period::Week(r.get(17)?),
                RepeatUnit::Month if r.get(26)? => Period::LunarMonth(r.get(20)?),
                RepeatUnit::Month => Period::Month(r.get(20)?),
            };
            Some(Repeat { every, period, from, until: r.get(25)? })
        }
        _ => None,
    };
    let completions = match (r.get(18)?, r.get(19)?) {
        (Some(count), Some(last)) => Some(Completions { count, last }),
        _ => None,
    };
    let due_after: Option<String> = r.get(27)?;
    let assignees: String = r.get(29)?;
    Ok(Task {
        id: r.get(0)?,
        number: r.get(1)?,
        folder_id: r.get(2)?,
        title: r.get(3)?,
        description: r.get(4)?,
        done_at: r.get(5)?,
        start: r.get(21)?,
        start_time: r.get(24)?,
        due_after: due_after
            .map(|w| serde_json::from_str(&w))
            .transpose()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(27, rusqlite::types::Type::Text, Box::new(e)))?,
        unlocked_on: r.get(28)?,
        due: date
            .map(|date| Due::new(date, time, reminders, repeat))
            .transpose()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into()))?,
        completions,
        sort_key: r.get(9)?,
        assignees: serde_json::from_str(&assignees)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(29, rusqlite::types::Type::Text, Box::new(e)))?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        created_by: r.get(22)?,
        updated_by: r.get(23)?,
        created_via: r.get(12)?,
        updated_via: r.get(13)?,
    })
}

fn job_row(r: &Row) -> rusqlite::Result<AiJob> {
    let suggestions: Option<String> = r.get(7)?;
    let reminders: Option<String> = r.get(10)?;
    Ok(AiJob {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        kind: r.get(2)?,
        text: r.get(3)?,
        local_time: r.get(4)?,
        state: r.get(5)?,
        error: r.get(6)?,
        suggestions: suggestions
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e)))?,
        reminders: reminders
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e)))?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

fn suggestions(j: &AiJob) -> Option<String> {
    j.suggestions.as_ref().map(|s| serde_json::to_string(s).expect("suggestions serialize"))
}

fn reminders(j: &AiJob) -> Option<String> {
    j.reminders.as_ref().map(|r| serde_json::to_string(r).expect("reminders serialize"))
}

fn dep_row(r: &Row) -> rusqlite::Result<Dependency> {
    Ok(Dependency { before: r.get(0)?, after: r.get(1)?, created_at: r.get(2)? })
}

fn completion_row(r: &Row) -> rusqlite::Result<Completion> {
    Ok(Completion { task_id: r.get(0)?, due: r.get(1)?, done: r.get(2)?, again: r.get(3)? })
}

fn attachment_row(r: &Row) -> rusqlite::Result<Attachment> {
    // One of the two, as the table checks.
    let holder = match r.get::<_, Option<TaskId>>(1)? {
        Some(t) => Holder::Task(t),
        None => Holder::Job(r.get(2)?),
    };
    Ok(Attachment { id: r.get(0)?, holder, name: r.get(3)?, media_type: r.get(4)?, size: r.get(5)?, created_at: r.get(6)? })
}

fn holder_columns(h: Holder) -> (Option<TaskId>, Option<AiJobId>) {
    match h {
        Holder::Task(t) => (Some(t), None),
        Holder::Job(j) => (None, Some(j)),
    }
}

fn all<T>(c: &Connection, sql: &str, p: impl Params, f: fn(&Row) -> rusqlite::Result<T>) -> rusqlite::Result<Vec<T>> {
    c.prepare_cached(sql)?.query_map(p, f)?.collect()
}

/// The due date's columns: date, time, reminders as JSON, and the repeat's every, unit, from, days of the week,
/// days of the month, last day and whether it goes by the lunar calendar.
type DueColumns<'a> = (
    Option<&'a Date>,
    Option<&'a Time>,
    Option<String>,
    Option<Every>,
    Option<RepeatUnit>,
    Option<RepeatFrom>,
    Option<Weekdays>,
    Option<Monthdays>,
    Option<&'a Date>,
    bool,
);

fn due_columns(due: &Option<Due>) -> DueColumns<'_> {
    let Some(d) = due else { return (None, None, None, None, None, None, None, None, None, false) };
    let at = d.at.as_ref();
    let r = d.repeat.as_ref();
    let (days, monthdays) = match r.map(|r| r.period) {
        Some(Period::Week(days)) => (days, None),
        Some(Period::Month(days)) => (None, days),
        Some(Period::LunarMonth(days)) => (None, Some(days)),
        _ => (None, None),
    };
    let (every, unit, from) = (r.map(|r| r.every), r.map(|r| r.period.unit()), r.map(|r| r.from));
    let until = r.and_then(|r| r.until.as_ref());
    let lunar = r.is_some_and(|r| matches!(r.period, Period::LunarMonth(_)));
    let reminders = d.reminders();
    let reminders = (!reminders.is_empty()).then(|| serde_json::to_string(&reminders).expect("reminders are JSON"));
    (Some(&d.date), at.map(|at| &at.time), reminders, every, unit, from, days, monthdays, until, lunar)
}

/// A folder and its owner.
pub fn load_folder(c: &Connection, id: FolderId) -> rusqlite::Result<Option<(Folder, String)>> {
    c.query_row(&format!("SELECT {FOLDER_COLUMNS}, owner_id FROM folders WHERE id = ?1"), [id], |r| Ok((folder_row(r)?, r.get(8)?)))
        .optional()
}

/// Whether another folder next to `f` already has its name.
pub fn folder_name_taken(c: &Connection, owner: &str, f: &Folder) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM folders WHERE owner_id = ?1 AND parent_id IS ?2 AND name = ?3 AND id <> ?4)",
        params![owner, f.parent_id, f.name, f.id],
        |r| r.get(0),
    )
}

pub fn folder_id_taken(c: &Connection, id: FolderId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM folders WHERE id = ?1)", [id], |r| r.get(0))
}

pub fn insert_folder(c: &Connection, owner: &str, f: &Folder) -> rusqlite::Result<()> {
    // Its owner's, as is everything in it: one shared with others too.
    c.execute(
        "INSERT INTO folders (owner_id, id, parent_id, name, sort_key, created_at, updated_at, color, icon) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![owner, f.id, f.parent_id, f.name, f.sort_key, f.created_at, f.updated_at, f.color, f.icon],
    )?;
    Ok(())
}

pub fn update_folder(c: &Connection, f: &Folder) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE folders SET parent_id = ?2, name = ?3, sort_key = ?4, updated_at = ?5, color = ?6, icon = ?7 WHERE id = ?1",
        params![f.id, f.parent_id, f.name, f.sort_key, f.updated_at, f.color, f.icon],
    )?;
    Ok(())
}

/// Deletes the folder; its tasks, with their dependencies and completion records, go with it (foreign
/// key cascade).
pub fn delete_folder(c: &Connection, id: FolderId) -> rusqlite::Result<()> {
    c.execute("DELETE FROM folders WHERE id = ?1", [id])?;
    Ok(())
}

/// Folders in the subtree rooted at `id`, the deepest first.
pub fn subtree_folders(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<Folder>> {
    all(
        c,
        &format!(
            "{SUBTREE} SELECT f.id, f.parent_id, f.name, f.sort_key, f.created_at, f.updated_at, f.color, f.icon
             FROM folders f JOIN sub ON f.id = sub.id ORDER BY sub.depth DESC"
        ),
        [id],
        folder_row,
    )
}

pub fn subtree_tasks(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<Task>> {
    all(c, &format!("{SUBTREE} SELECT {TASK_SELECT} FROM tasks WHERE folder_id IN (SELECT id FROM sub)"), [id], task_row)
}

pub fn subtree_dependencies(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<Dependency>> {
    all(
        c,
        &format!(
            "{SUBTREE}, t AS (SELECT id FROM tasks WHERE folder_id IN (SELECT id FROM sub))
             SELECT before_id, after_id, created_at FROM dependencies WHERE before_id IN t OR after_id IN t"
        ),
        [id],
        dep_row,
    )
}

/// Gives everything in the subtree rooted at `id` to `owner`: the folders, and the dependencies
/// between the tasks there. The tasks go one by one, as their numbers change.
pub fn give_subtree(c: &Connection, id: FolderId, owner: &str) -> rusqlite::Result<()> {
    c.execute(&format!("{SUBTREE} UPDATE folders SET owner_id = ?2 WHERE id IN (SELECT id FROM sub)"), params![id, owner])?;
    c.execute(
        &format!(
            "{SUBTREE}, t AS (SELECT id FROM tasks WHERE folder_id IN (SELECT id FROM sub))
             UPDATE dependencies SET owner_id = ?2 WHERE before_id IN t AND after_id IN t"
        ),
        params![id, owner],
    )?;
    Ok(())
}

pub fn in_subtree(c: &Connection, root: FolderId, id: FolderId) -> rusqlite::Result<bool> {
    c.query_row(&format!("{SUBTREE} SELECT EXISTS (SELECT 1 FROM sub WHERE id = ?2)"), params![root, id], |r| r.get(0))
}

pub fn last_folder_key(c: &Connection, owner: &str, parent: Option<FolderId>) -> rusqlite::Result<Option<SortKey>> {
    c.query_row("SELECT max(sort_key) FROM folders WHERE owner_id = ?1 AND parent_id IS ?2", params![owner, parent], |r| r.get(0))
}

/// A task and its owner.
pub fn load_task(c: &Connection, id: TaskId) -> rusqlite::Result<Option<(Task, String)>> {
    c.query_row(&format!("SELECT {TASK_SELECT}, owner_id FROM tasks WHERE id = ?1"), [id], |r| Ok((task_row(r)?, r.get(30)?))).optional()
}

/// Whether `owner` has another task than `id` numbered `number`.
/// Whether `owner` has given out `number` before and no task of theirs but `id` has it now.
pub fn number_free(c: &Connection, owner: &str, number: i64, id: TaskId) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT ?2 < next_task_number AND NOT EXISTS (SELECT 1 FROM tasks WHERE owner_id = ?1 AND number = ?2 AND id <> ?3) FROM users WHERE id = ?1",
        params![owner, number, id],
        |r| r.get(0),
    )
}

pub fn task_id_taken(c: &Connection, id: TaskId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM tasks WHERE id = ?1)", [id], |r| r.get(0))
}

pub fn insert_task(c: &Connection, owner: &str, t: &Task) -> rusqlite::Result<()> {
    let (date, time, remind, every, unit, from, days, monthdays, until, lunar) = due_columns(&t.due);
    c.execute(
        &format!(
            "INSERT INTO tasks (owner_id, {TASK_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)"
        ),
        params![
            owner,
            t.id,
            t.number,
            t.folder_id,
            t.title,
            t.description,
            t.done_at,
            date,
            time,
            remind,
            t.sort_key,
            t.created_at,
            t.updated_at,
            t.created_via,
            t.updated_via,
            every,
            unit,
            from,
            days,
            t.completions.as_ref().map(|c| c.count),
            t.completions.as_ref().map(|c| &c.last),
            monthdays,
            t.start,
            t.created_by,
            t.updated_by,
            t.start_time,
            until,
            lunar,
            due_after(t),
            t.unlocked_on
        ],
    )?;
    set_assignees(c, t)
}

pub fn update_task(c: &Connection, owner: &str, t: &Task) -> rusqlite::Result<()> {
    let (date, time, remind, every, unit, from, days, monthdays, until, lunar) = due_columns(&t.due);
    c.execute(
        "UPDATE tasks SET folder_id = ?2, title = ?3, description = ?4, done_at = ?5, due_date = ?6, due_time = ?7,
         reminders = ?8, sort_key = ?9, updated_at = ?10, updated_via = ?11, repeat_every = ?12, repeat_unit = ?13,
         repeat_from = ?14, repeat_days = ?15, completion_count = ?16, last_completed = ?17, repeat_monthdays = ?18,
         start_date = ?19, updated_by = ?20, owner_id = ?21, number = ?22, start_time = ?23, repeat_until = ?24, repeat_lunar = ?25,
         due_after = ?26, unlocked_on = ?27 WHERE id = ?1",
        params![
            t.id,
            t.folder_id,
            t.title,
            t.description,
            t.done_at,
            date,
            time,
            remind,
            t.sort_key,
            t.updated_at,
            t.updated_via,
            every,
            unit,
            from,
            days,
            t.completions.as_ref().map(|c| c.count),
            t.completions.as_ref().map(|c| &c.last),
            monthdays,
            t.start,
            t.updated_by,
            owner,
            t.number,
            t.start_time,
            until,
            lunar,
            due_after(t),
            t.unlocked_on
        ],
    )?;
    set_assignees(c, t)
}

fn due_after(t: &Task) -> Option<String> {
    t.due_after.as_ref().map(|a| serde_json::to_string(a).expect("due_after serializes"))
}

fn set_assignees(c: &Connection, t: &Task) -> rusqlite::Result<()> {
    c.execute("DELETE FROM assignees WHERE task_id = ?1", [t.id])?;
    for user in &t.assignees {
        c.execute("INSERT INTO assignees (task_id, user_id) VALUES (?1, ?2)", params![t.id, user])?;
    }
    Ok(())
}

/// Deletes the task; its dependencies and completion records go with it (foreign key cascade).
pub fn delete_task(c: &Connection, id: TaskId) -> rusqlite::Result<()> {
    c.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
    Ok(())
}

pub fn last_task_key(c: &Connection, owner: &str, folder: Option<FolderId>) -> rusqlite::Result<Option<SortKey>> {
    c.query_row("SELECT max(sort_key) FROM tasks WHERE owner_id = ?1 AND folder_id IS ?2", params![owner, folder], |r| r.get(0))
}

pub fn take_task_number(c: &Connection, owner: &str) -> rusqlite::Result<i64> {
    c.query_row("UPDATE users SET next_task_number = next_task_number + 1 WHERE id = ?1 RETURNING next_task_number - 1", [owner], |r| {
        r.get(0)
    })
}

pub fn load_dependency(c: &Connection, before: TaskId, after: TaskId) -> rusqlite::Result<Option<Dependency>> {
    c.query_row(
        "SELECT before_id, after_id, created_at FROM dependencies WHERE before_id = ?1 AND after_id = ?2",
        params![before, after],
        dep_row,
    )
    .optional()
}

pub fn task_dependencies(c: &Connection, id: TaskId) -> rusqlite::Result<Vec<Dependency>> {
    all(c, "SELECT before_id, after_id, created_at FROM dependencies WHERE before_id = ?1 OR after_id = ?1", [id], dep_row)
}

pub fn dependency_edges(c: &Connection, owner: &str) -> rusqlite::Result<Vec<(TaskId, TaskId)>> {
    c.prepare_cached("SELECT before_id, after_id FROM dependencies WHERE owner_id = ?1")?
        .query_map([owner], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

pub fn insert_dependency(c: &Connection, owner: &str, d: &Dependency) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO dependencies (before_id, after_id, owner_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![d.before, d.after, owner, d.created_at],
    )?;
    Ok(())
}

pub fn delete_dependency(c: &Connection, d: &Dependency) -> rusqlite::Result<()> {
    c.execute("DELETE FROM dependencies WHERE before_id = ?1 AND after_id = ?2", params![d.before, d.after])?;
    Ok(())
}

/// The record of the time task `task` was due on `due`, done that time for the `again`th time over.
pub fn load_completion(c: &Connection, task: TaskId, due: &Date, again: u32) -> rusqlite::Result<Option<Completion>> {
    c.query_row(
        "SELECT task_id, due_date, done_on, again FROM completions WHERE task_id = ?1 AND due_date = ?2 AND again = ?3",
        params![task, due, again],
        completion_row,
    )
    .optional()
}

/// The `again` of a new record of the time task `task` was due on `due`: after those it has.
pub fn next_again(c: &Connection, task: TaskId, due: &Date) -> rusqlite::Result<u32> {
    c.query_row("SELECT coalesce(max(again) + 1, 0) FROM completions WHERE task_id = ?1 AND due_date = ?2", params![task, due], |r| {
        r.get(0)
    })
}

pub fn task_completions(c: &Connection, task: TaskId) -> rusqlite::Result<Vec<Completion>> {
    all(c, "SELECT task_id, due_date, done_on, again FROM completions WHERE task_id = ?1 ORDER BY due_date, again", [task], completion_row)
}

/// The records of the tasks in the subtree rooted at `id`.
pub fn subtree_completions(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<Completion>> {
    all(
        c,
        &format!(
            "{SUBTREE} SELECT task_id, due_date, done_on, again FROM completions
             WHERE task_id IN (SELECT id FROM tasks WHERE folder_id IN (SELECT id FROM sub)) ORDER BY task_id, due_date, again"
        ),
        [id],
        completion_row,
    )
}

pub fn insert_completion(c: &Connection, r: &Completion) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO completions (task_id, due_date, again, done_on) VALUES (?1, ?2, ?3, ?4)",
        params![r.task_id, r.due, r.again, r.done],
    )?;
    Ok(())
}

pub fn delete_completion(c: &Connection, r: &Completion) -> rusqlite::Result<()> {
    c.execute("DELETE FROM completions WHERE task_id = ?1 AND due_date = ?2 AND again = ?3", params![r.task_id, r.due, r.again])?;
    Ok(())
}

/// The AI jobs in the subtree rooted at `id`, with whose they are.
pub fn subtree_jobs(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<(AiJob, String)>> {
    c.prepare_cached(&format!("{SUBTREE} SELECT {JOB_COLUMNS}, owner_id FROM ai_jobs WHERE folder_id IN (SELECT id FROM sub)"))?
        .query_map([id], |r| Ok((job_row(r)?, r.get(11)?)))?
        .collect()
}

/// The user's notes for AI organizing, by folder, the root's `None` (docs/ai.md#说明); those of folders
/// they no longer see too.
pub fn ai_notes(c: &Connection, user: &str) -> rusqlite::Result<HashMap<Option<FolderId>, NoteText>> {
    c.prepare("SELECT folder_id, text FROM ai_notes WHERE user_id = ?1")?.query_map([user], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

/// Puts `text` in place of the user's note for `folder`; an empty one clears it.
pub fn set_ai_note(c: &Connection, user: &str, folder: Option<FolderId>, text: &NoteText, now: i64) -> rusqlite::Result<()> {
    if text.as_str().is_empty() {
        c.execute("DELETE FROM ai_notes WHERE user_id = ?1 AND folder_id IS ?2", params![user, folder])?;
    } else {
        c.execute(
            "INSERT INTO ai_notes (user_id, folder_id, text, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (user_id, ifnull(folder_id, '')) DO UPDATE SET text = ?3, updated_at = ?4",
            params![user, folder, text, now],
        )?;
    }
    Ok(())
}

pub fn load_job(c: &Connection, owner: &str, id: AiJobId) -> rusqlite::Result<Option<AiJob>> {
    c.query_row(&format!("SELECT {JOB_COLUMNS} FROM ai_jobs WHERE id = ?1 AND owner_id = ?2"), params![id, owner], job_row).optional()
}

pub fn job_id_taken(c: &Connection, id: AiJobId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM ai_jobs WHERE id = ?1)", [id], |r| r.get(0))
}

pub fn insert_job(c: &Connection, owner: &str, j: &AiJob) -> rusqlite::Result<()> {
    c.execute(
        &format!("INSERT INTO ai_jobs (owner_id, {JOB_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"),
        params![
            owner,
            j.id,
            j.folder_id,
            j.kind,
            j.text,
            j.local_time,
            j.state,
            j.error,
            suggestions(j),
            j.created_at,
            j.updated_at,
            reminders(j)
        ],
    )?;
    Ok(())
}

pub fn update_job(c: &Connection, j: &AiJob) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE ai_jobs SET folder_id = ?2, text = ?3, state = ?4, error = ?5, suggestions = ?6, updated_at = ?7, reminders = ?8 WHERE id = ?1",
        params![j.id, j.folder_id, j.text, j.state, j.error, suggestions(j), j.updated_at, reminders(j)],
    )?;
    Ok(())
}

pub fn delete_job(c: &Connection, id: AiJobId) -> rusqlite::Result<()> {
    c.execute("DELETE FROM ai_jobs WHERE id = ?1", [id])?;
    Ok(())
}

pub fn load_attachment(c: &Connection, id: AttachmentId) -> rusqlite::Result<Option<Attachment>> {
    c.query_row(&format!("{ATTACHMENTS} WHERE a.id = ?1"), [id], attachment_row).optional()
}

/// A task's attachments, in the order they were added.
pub fn task_attachments(c: &Connection, task: TaskId) -> rusqlite::Result<Vec<Attachment>> {
    all(c, &format!("{ATTACHMENTS} WHERE a.task_id = ?1 ORDER BY a.id"), [task], attachment_row)
}

pub fn job_attachments(c: &Connection, job: AiJobId) -> rusqlite::Result<Vec<Attachment>> {
    all(c, &format!("{ATTACHMENTS} WHERE a.job_id = ?1 ORDER BY a.id"), [job], attachment_row)
}

/// The attachments of the tasks in the subtree rooted at `id`.
pub fn subtree_attachments(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<Attachment>> {
    all(
        c,
        &format!(
            "{SUBTREE} {ATTACHMENTS}
             WHERE a.task_id IN (SELECT id FROM tasks WHERE folder_id IN (SELECT id FROM sub)) ORDER BY a.id"
        ),
        [id],
        attachment_row,
    )
}

/// Attaches its file, which is then in use.
pub fn insert_attachment(c: &Connection, a: &Attachment) -> rusqlite::Result<()> {
    let (task, job) = holder_columns(a.holder);
    c.execute(
        "INSERT INTO attachments (id, task_id, job_id, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![a.id, task, job, a.name, a.created_at],
    )?;
    c.execute("UPDATE files SET unused_at = NULL WHERE id = ?1", [a.id])?;
    Ok(())
}

pub fn update_attachment(c: &Connection, a: &Attachment) -> rusqlite::Result<()> {
    let (task, job) = holder_columns(a.holder);
    c.execute("UPDATE attachments SET task_id = ?2, job_id = ?3, name = ?4 WHERE id = ?1", params![a.id, task, job, a.name])?;
    Ok(())
}

/// Deletes the attachment; its file is unused from `now` on, kept for undoing it.
pub fn delete_attachment(c: &Connection, id: AttachmentId, now: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM attachments WHERE id = ?1", [id])?;
    c.execute("UPDATE files SET unused_at = ?2 WHERE id = ?1", params![id, now])?;
    Ok(())
}

/// A file uploaded, as recorded.
pub struct StoredFile {
    pub owner: String,
    pub media_type: MediaType,
    pub size: FileSize,
}

/// The file `id`, none once it was cleared through a drive (docs/local.md#附件).
pub fn load_file(c: &Connection, id: AttachmentId) -> rusqlite::Result<Option<StoredFile>> {
    c.query_row("SELECT owner_id, type, size FROM files WHERE id = ?1 AND NOT purged", [id], |r| {
        Ok(StoredFile { owner: r.get(0)?, media_type: r.get(1)?, size: r.get(2)? })
    })
    .optional()
}

/// Records a file just uploaded, not attached yet; `staged` on this device alone, syncing through a
/// drive, until it is attached in a commit.
pub fn insert_file(c: &Connection, id: AttachmentId, f: &StoredFile, now: i64, staged: bool) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO files (id, owner_id, type, size, created_at, unused_at, staged) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
        params![id, f.owner, f.media_type, f.size, now, staged],
    )?;
    Ok(())
}

/// The files not cleared, by id with their sizes: those in use, and those no longer, which undoing may
/// bring back until they are.
pub fn kept_files(c: &Connection) -> rusqlite::Result<Vec<(AttachmentId, FileSize)>> {
    c.prepare("SELECT id, size FROM files WHERE NOT purged")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

/// Whether file `id` was cleared through a drive: its id can't be used again.
pub fn file_purged(c: &Connection, id: AttachmentId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM files WHERE id = ?1 AND purged)", [id], |r| r.get(0))
}

pub fn attachment_id_taken(c: &Connection, id: AttachmentId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM attachments WHERE id = ?1)", [id], |r| r.get(0))
}

/// Whether `user` sees task `id`: theirs, or in a folder shared with them.
pub fn sees_task(c: &Connection, user: &str, id: TaskId) -> rusqlite::Result<bool> {
    c.query_row(
        &format!("{SHARED} SELECT EXISTS (SELECT 1 FROM tasks WHERE id = ?2 AND (owner_id = ?1 OR folder_id IN shared))"),
        params![user, id],
        |r| r.get(0),
    )
}

/// Whether job `id` is `user`'s.
pub fn owns_job(c: &Connection, user: &str, id: AiJobId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM ai_jobs WHERE id = ?1 AND owner_id = ?2)", params![id, user], |r| r.get(0))
}

/// The shared folder `id` is in, or is.
pub fn shared_root(c: &Connection, id: FolderId) -> rusqlite::Result<Option<FolderId>> {
    c.query_row(
        "WITH RECURSIVE up(id, depth) AS (
             SELECT ?1, 0 UNION ALL SELECT f.parent_id, up.depth + 1 FROM folders f JOIN up ON f.id = up.id WHERE f.parent_id IS NOT NULL)
         SELECT id FROM up WHERE EXISTS (SELECT 1 FROM members WHERE folder_id = up.id) ORDER BY depth LIMIT 1",
        [id],
        |r| r.get(0),
    )
    .optional()
}

/// The shared folders in the subtree rooted at `id`, it included.
pub fn shared_within(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<FolderId>> {
    let mut stmt =
        c.prepare_cached(&format!("{SUBTREE} SELECT id FROM sub WHERE EXISTS (SELECT 1 FROM members WHERE folder_id = sub.id)"))?;
    stmt.query_map([id], |r| r.get(0))?.collect()
}

/// Whether `owner` shares another folder than `id` named `name`.
pub fn shared_name_taken(c: &Connection, owner: &str, name: &str, id: FolderId) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM folders f WHERE owner_id = ?1 AND name = ?2 AND id <> ?3
             AND EXISTS (SELECT 1 FROM members WHERE folder_id = f.id))",
        params![owner, name, id],
        |r| r.get(0),
    )
}

/// Who a folder is shared with, by user id in order.
pub fn members(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<String>> {
    c.prepare_cached("SELECT user_id FROM members WHERE folder_id = ?1 ORDER BY user_id")?.query_map([id], |r| r.get(0))?.collect()
}

pub fn insert_member(c: &Connection, id: FolderId, user: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO members (folder_id, user_id) VALUES (?1, ?2)", params![id, user])?;
    Ok(())
}

pub fn delete_member(c: &Connection, id: FolderId, user: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM members WHERE folder_id = ?1 AND user_id = ?2", params![id, user])?;
    Ok(())
}

fn person_row(r: &Row) -> rusqlite::Result<Person> {
    Ok(Person { id: r.get(0)?, username: r.get(1)?, display_name: r.get(2)? })
}

/// The people with these ids.
pub fn people(c: &Connection, ids: &[String]) -> rusqlite::Result<Vec<Person>> {
    ids.iter().map(|id| person(c, id)).collect()
}

pub fn person(c: &Connection, id: &str) -> rusqlite::Result<Person> {
    c.query_row("SELECT id, username, display_name FROM users WHERE id = ?1", [id], person_row)
}

/// Someone by the username they sign in with, whatever its case.
pub fn person_named(c: &Connection, username: &str) -> rusqlite::Result<Option<Person>> {
    c.query_row("SELECT id, username, display_name FROM users WHERE username = ?1", [username.trim()], person_row).optional()
}

/// Who else has `user` among the people of their snapshot: those sharing a folder with `user`, and
/// those who see a task `user` made or last changed.
pub fn known_by(c: &Connection, user: &str) -> rusqlite::Result<Vec<String>> {
    c.prepare_cached(
        "WITH RECURSIVE s(id, owner) AS (SELECT m.folder_id, f.owner_id FROM members m JOIN folders f ON f.id = m.folder_id
             WHERE f.owner_id = ?1 OR m.user_id = ?1),
         made(owner, folder) AS (SELECT owner_id, folder_id FROM tasks WHERE created_by = ?1 OR updated_by = ?1),
         up(id) AS (SELECT folder FROM made WHERE folder IS NOT NULL
             UNION SELECT f.parent_id FROM folders f JOIN up ON f.id = up.id WHERE f.parent_id IS NOT NULL)
         SELECT owner FROM s UNION SELECT m.user_id FROM members m JOIN s ON m.folder_id = s.id
         UNION SELECT owner FROM made UNION SELECT m.user_id FROM members m JOIN up ON m.folder_id = up.id
         EXCEPT SELECT ?1",
    )?
    .query_map([user], |r| r.get(0))?
    .collect()
}

/// All a user can see, in id order: their own data, and the folders shared with them, which there
/// have no parent.
pub fn snapshot(c: &Connection, user: &str) -> rusqlite::Result<Snapshot> {
    let shares: Vec<(FolderId, String)> = c
        .prepare_cached(
            "SELECT DISTINCT m.folder_id, f.owner_id FROM members m JOIN folders f ON f.id = m.folder_id
             WHERE f.owner_id = ?1 OR m.folder_id IN (SELECT folder_id FROM members WHERE user_id = ?1) ORDER BY m.folder_id",
        )?
        .query_map([user], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let shares = shares
        .into_iter()
        .map(|(folder_id, owner)| Ok(Share { members: members(c, folder_id)?, folder_id, owner }))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let visible = "owner_id = ?1 OR folder_id IN shared";
    let tasks: Vec<Task> = all(c, &format!("{SHARED} SELECT {TASK_SELECT} FROM tasks WHERE {visible} ORDER BY id"), [user], task_row)?;
    let mut people: Vec<&str> = shares
        .iter()
        .flat_map(|s| std::iter::once(&s.owner).chain(&s.members))
        .chain(tasks.iter().flat_map(|t| [&t.created_by, &t.updated_by]))
        .map(String::as_str)
        .collect();
    people.push(user);
    people.sort();
    people.dedup();
    let people = people.into_iter().map(|id| person(c, id)).collect::<rusqlite::Result<_>>()?;
    Ok(Snapshot {
        folders: all(
            c,
            &format!(
                "{SHARED} SELECT id, CASE WHEN id IN (SELECT folder_id FROM members WHERE user_id = ?1) THEN NULL ELSE parent_id END,
                     name, sort_key, created_at, updated_at, color, icon
                 FROM folders WHERE owner_id = ?1 OR id IN shared ORDER BY id"
            ),
            [user],
            folder_row,
        )?,
        tasks,
        dependencies: all(
            c,
            &format!(
                "{SHARED} SELECT before_id, after_id, created_at FROM dependencies
                 WHERE before_id IN (SELECT id FROM tasks WHERE {visible}) ORDER BY before_id, after_id"
            ),
            [user],
            dep_row,
        )?,
        completions: all(
            c,
            &format!(
                "{SHARED} SELECT task_id, due_date, done_on, again FROM completions
                 WHERE task_id IN (SELECT id FROM tasks WHERE {visible}) ORDER BY task_id, due_date, again"
            ),
            [user],
            completion_row,
        )?,
        attachments: all(
            c,
            &format!(
                "{SHARED} {ATTACHMENTS}
                 WHERE a.task_id IN (SELECT id FROM tasks WHERE {visible}) OR a.job_id IN (SELECT id FROM ai_jobs WHERE owner_id = ?1)
                 ORDER BY a.id"
            ),
            [user],
            attachment_row,
        )?,
        ai_jobs: all(c, &format!("SELECT {JOB_COLUMNS} FROM ai_jobs WHERE owner_id = ?1 ORDER BY id"), [user], job_row)?,
        shares,
        people,
    })
}

/// The latest mutation sequence number: every change a user is to see up to it has been sent to them,
/// or will download with their data.
pub fn latest_seq(c: &Connection, user: &str) -> rusqlite::Result<i64> {
    c.query_row("SELECT max(pruned_seq, coalesce((SELECT max(seq) FROM mutations), 0)) FROM users WHERE id = ?1", [user], |r| r.get(0))
}
