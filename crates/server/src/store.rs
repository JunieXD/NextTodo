//! Reading and writing entities. Every lookup is scoped to the owner.

use nexttodo_core::board::Snapshot;
use nexttodo_core::model::{AiJob, AiJobId, Date, Dependency, Due, DueAt, Folder, FolderId, SortKey, Task, TaskId, Time};
use rusqlite::{Connection, OptionalExtension, Params, Row, params};

const FOLDER_COLUMNS: &str = "id, parent_id, name, sort_key, created_at, updated_at";
const JOB_COLUMNS: &str = "id, folder_id, text, local_time, state, error, created_at, updated_at";
const TASK_COLUMNS: &str = "id, number, folder_id, title, description, done_at, due_date, due_time, remind_before, sort_key, created_at, updated_at, created_via, updated_via";

/// Folder ids in the subtree rooted at `?1`, including the root.
const SUBTREE: &str = "WITH RECURSIVE sub(id, depth) AS (
    SELECT ?1, 0 UNION ALL SELECT f.id, sub.depth + 1 FROM folders f JOIN sub ON f.parent_id = sub.id)";

fn folder_row(r: &Row) -> rusqlite::Result<Folder> {
    Ok(Folder { id: r.get(0)?, parent_id: r.get(1)?, name: r.get(2)?, sort_key: r.get(3)?, created_at: r.get(4)?, updated_at: r.get(5)? })
}

fn task_row(r: &Row) -> rusqlite::Result<Task> {
    let date: Option<Date> = r.get(6)?;
    let time: Option<Time> = r.get(7)?;
    let remind_before: Option<u32> = r.get(8)?;
    Ok(Task {
        id: r.get(0)?,
        number: r.get(1)?,
        folder_id: r.get(2)?,
        title: r.get(3)?,
        description: r.get(4)?,
        done_at: r.get(5)?,
        due: date.map(|date| Due { date, at: time.map(|time| DueAt { time, remind_before }) }),
        sort_key: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        created_via: r.get(12)?,
        updated_via: r.get(13)?,
    })
}

fn job_row(r: &Row) -> rusqlite::Result<AiJob> {
    Ok(AiJob {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        text: r.get(2)?,
        local_time: r.get(3)?,
        state: r.get(4)?,
        error: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

fn dep_row(r: &Row) -> rusqlite::Result<Dependency> {
    Ok(Dependency { before: r.get(0)?, after: r.get(1)?, created_at: r.get(2)? })
}

fn all<T>(c: &Connection, sql: &str, p: impl Params, f: fn(&Row) -> rusqlite::Result<T>) -> rusqlite::Result<Vec<T>> {
    c.prepare_cached(sql)?.query_map(p, f)?.collect()
}

fn due_columns(due: &Option<Due>) -> (Option<&Date>, Option<&Time>, Option<u32>) {
    match due {
        None => (None, None, None),
        Some(Due { date, at: None }) => (Some(date), None, None),
        Some(Due { date, at: Some(at) }) => (Some(date), Some(&at.time), at.remind_before),
    }
}

pub fn load_folder(c: &Connection, owner: &str, id: FolderId) -> rusqlite::Result<Option<Folder>> {
    c.query_row(&format!("SELECT {FOLDER_COLUMNS} FROM folders WHERE id = ?1 AND owner_id = ?2"), params![id, owner], folder_row).optional()
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
    c.execute(
        "INSERT INTO folders (owner_id, id, parent_id, name, sort_key, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![owner, f.id, f.parent_id, f.name, f.sort_key, f.created_at, f.updated_at],
    )?;
    Ok(())
}

pub fn update_folder(c: &Connection, f: &Folder) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE folders SET parent_id = ?2, name = ?3, sort_key = ?4, updated_at = ?5 WHERE id = ?1",
        params![f.id, f.parent_id, f.name, f.sort_key, f.updated_at],
    )?;
    Ok(())
}

/// Deletes the folder; its tasks and their dependencies go with it (foreign key cascade).
pub fn delete_folder(c: &Connection, id: FolderId) -> rusqlite::Result<()> {
    c.execute("DELETE FROM folders WHERE id = ?1", [id])?;
    Ok(())
}

/// Folders in the subtree rooted at `id`, the deepest first.
pub fn subtree_folders(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<Folder>> {
    all(
        c,
        &format!(
            "{SUBTREE} SELECT f.id, f.parent_id, f.name, f.sort_key, f.created_at, f.updated_at
             FROM folders f JOIN sub ON f.id = sub.id ORDER BY sub.depth DESC"
        ),
        [id],
        folder_row,
    )
}

pub fn subtree_tasks(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<Task>> {
    all(c, &format!("{SUBTREE} SELECT {TASK_COLUMNS} FROM tasks WHERE folder_id IN (SELECT id FROM sub)"), [id], task_row)
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

pub fn in_subtree(c: &Connection, root: FolderId, id: FolderId) -> rusqlite::Result<bool> {
    c.query_row(&format!("{SUBTREE} SELECT EXISTS (SELECT 1 FROM sub WHERE id = ?2)"), params![root, id], |r| r.get(0))
}

pub fn last_folder_key(c: &Connection, owner: &str, parent: Option<FolderId>) -> rusqlite::Result<Option<SortKey>> {
    c.query_row("SELECT max(sort_key) FROM folders WHERE owner_id = ?1 AND parent_id IS ?2", params![owner, parent], |r| r.get(0))
}

pub fn load_task(c: &Connection, owner: &str, id: TaskId) -> rusqlite::Result<Option<Task>> {
    c.query_row(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1 AND owner_id = ?2"), params![id, owner], task_row).optional()
}

pub fn task_id_taken(c: &Connection, id: TaskId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM tasks WHERE id = ?1)", [id], |r| r.get(0))
}

pub fn insert_task(c: &Connection, owner: &str, t: &Task) -> rusqlite::Result<()> {
    let (date, time, remind) = due_columns(&t.due);
    c.execute(
        &format!("INSERT INTO tasks (owner_id, {TASK_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"),
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
            t.updated_via
        ],
    )?;
    Ok(())
}

pub fn update_task(c: &Connection, t: &Task) -> rusqlite::Result<()> {
    let (date, time, remind) = due_columns(&t.due);
    c.execute(
        "UPDATE tasks SET folder_id = ?2, title = ?3, description = ?4, done_at = ?5, due_date = ?6, due_time = ?7,
         remind_before = ?8, sort_key = ?9, updated_at = ?10, updated_via = ?11 WHERE id = ?1",
        params![t.id, t.folder_id, t.title, t.description, t.done_at, date, time, remind, t.sort_key, t.updated_at, t.updated_via],
    )?;
    Ok(())
}

/// Deletes the task; its dependencies go with it (foreign key cascade).
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

pub fn load_dependency(c: &Connection, owner: &str, before: TaskId, after: TaskId) -> rusqlite::Result<Option<Dependency>> {
    c.query_row(
        "SELECT before_id, after_id, created_at FROM dependencies WHERE before_id = ?1 AND after_id = ?2 AND owner_id = ?3",
        params![before, after, owner],
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

pub fn load_job(c: &Connection, owner: &str, id: AiJobId) -> rusqlite::Result<Option<AiJob>> {
    c.query_row(&format!("SELECT {JOB_COLUMNS} FROM ai_jobs WHERE id = ?1 AND owner_id = ?2"), params![id, owner], job_row).optional()
}

pub fn job_id_taken(c: &Connection, id: AiJobId) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM ai_jobs WHERE id = ?1)", [id], |r| r.get(0))
}

pub fn insert_job(c: &Connection, owner: &str, j: &AiJob) -> rusqlite::Result<()> {
    c.execute(
        &format!("INSERT INTO ai_jobs (owner_id, {JOB_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"),
        params![owner, j.id, j.folder_id, j.text, j.local_time, j.state, j.error, j.created_at, j.updated_at],
    )?;
    Ok(())
}

pub fn update_job(c: &Connection, j: &AiJob) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE ai_jobs SET folder_id = ?2, text = ?3, state = ?4, error = ?5, updated_at = ?6 WHERE id = ?1",
        params![j.id, j.folder_id, j.text, j.state, j.error, j.updated_at],
    )?;
    Ok(())
}

pub fn delete_job(c: &Connection, id: AiJobId) -> rusqlite::Result<()> {
    c.execute("DELETE FROM ai_jobs WHERE id = ?1", [id])?;
    Ok(())
}

pub fn subtree_jobs(c: &Connection, id: FolderId) -> rusqlite::Result<Vec<AiJob>> {
    all(c, &format!("{SUBTREE} SELECT {JOB_COLUMNS} FROM ai_jobs WHERE folder_id IN (SELECT id FROM sub)"), [id], job_row)
}

/// All of a user's data, in id order.
pub fn snapshot(c: &Connection, owner: &str) -> rusqlite::Result<Snapshot> {
    Ok(Snapshot {
        folders: all(c, &format!("SELECT {FOLDER_COLUMNS} FROM folders WHERE owner_id = ?1 ORDER BY id"), [owner], folder_row)?,
        tasks: all(c, &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE owner_id = ?1 ORDER BY id"), [owner], task_row)?,
        dependencies: all(
            c,
            "SELECT before_id, after_id, created_at FROM dependencies WHERE owner_id = ?1 ORDER BY before_id, after_id",
            [owner],
            dep_row,
        )?,
        ai_jobs: all(c, &format!("SELECT {JOB_COLUMNS} FROM ai_jobs WHERE owner_id = ?1 ORDER BY id"), [owner], job_row)?,
    })
}

/// The latest mutation sequence number the user can see.
pub fn latest_seq(c: &Connection, owner: &str) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT max(pruned_seq, coalesce((SELECT max(seq) FROM mutations WHERE owner_id = ?1), 0)) FROM users WHERE id = ?1",
        [owner],
        |r| r.get(0),
    )
}
