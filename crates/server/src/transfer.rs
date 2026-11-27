//! Moving data between local mode and a server account (docs/local/backup.md#搬到服务器和搬回本机): an
//! account's own data as a backup, and a backup added to an account. Local mode has its own backup
//! and restore instead.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::Path;

use axum::body::Body;
use axum::extract::{Query, Request, State};
use axum::http::{HeaderValue, header};
use axum::response::Response;
use http_body_util::BodyExt;
use nexttodo_core::board::Snapshot;
use nexttodo_core::dag::acyclic;
use nexttodo_core::model::{
    AiJob, AiJobId, Attachment, AttachmentId, Change, Completion, Date, Dependency, Description, Effect, Folder, FolderId, Holder,
    JobState, MutationId, NoteText, Task, TaskId,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use ts_rs::TS;
use uuid::Uuid;

use crate::attachments::encode;
use crate::auth::Auth;
use crate::backup::{self, NOT_OURS};
use crate::error::{ApiError, Json};
use crate::mutate::{self, Actor, Applied, Import};
use crate::{AppState, db, store};

#[derive(Deserialize)]
pub struct ExportQuery {
    /// The day on the device, for the file's name.
    today: Option<Date>,
}

/// The user's own data as a backup (docs/local/backup.md#导出).
pub async fn export(State(st): State<AppState>, auth: Auth, Query(q): Query<ExportQuery>) -> Result<Response, ApiError> {
    if st.local.is_some() {
        return Err(ApiError::not_found("接口不存在"));
    }
    let (data, tar) = (st.files.scratch()?, st.files.scratch()?);
    let path = data.path().to_owned();
    tokio::task::spawn_blocking(move || db::migrate(&mut Connection::open(path)?)).await.expect("migrating panicked").map_err(failed)?;
    let path = data.path().to_owned();
    let username = st
        .db
        .call(move |c| {
            c.execute("ATTACH ?1 AS out", [path.to_str().expect("data directory path is UTF-8")])?;
            let copied = copy_own(c, &auth.user_id);
            c.execute("DETACH out", [])?;
            copied
        })
        .await?;
    let (files, to) = (st.files.clone(), tar.path().to_owned());
    tokio::task::spawn_blocking(move || backup::tar(data.path(), &files, &to)).await.expect("packing panicked").map_err(failed)?;
    let Ok(res) = ServeFile::new(tar.path()).oneshot(Request::new(Body::empty())).await;
    // The file goes once it is sent, or the download is given up.
    let mut res = res.map(move |b| {
        Body::new(b.map_frame(move |f| {
            let _ = &tar;
            f
        }))
    });
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-tar"));
    let name = format!("NextTodo-{username}-{}.tar", q.today.unwrap_or_else(|| Date::utc(st.clock.now())).as_str());
    let disposition = format!("attachment; filename*=UTF-8''{}", encode(&name));
    h.insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition).expect("encoded to ASCII"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// Copies `user`'s own data to the database attached as `out`, empty and migrated as this one is, so
/// that the tables' columns are in the same order; returns their username.
fn copy_own(c: &mut Connection, user: &str) -> Result<String, ApiError> {
    let tx = c.transaction()?;
    for sql in [
        // No password goes with the data, not even in what the file has left over: never written there.
        "INSERT INTO out.users (id, username, display_name, password_hash, is_admin, next_task_number, pruned_seq, created_at, prefs)
         SELECT id, username, display_name, '', is_admin, next_task_number, 0, created_at, prefs FROM main.users WHERE id = ?1",
        // Theirs is what is in their folders and at their root, shared or not; what others share with
        // them is the others'.
        "INSERT INTO out.folders SELECT * FROM main.folders WHERE owner_id = ?1",
        "INSERT INTO out.tasks SELECT * FROM main.tasks WHERE owner_id = ?1",
        // The one user there: the members who added or changed some are not.
        "UPDATE out.tasks SET created_by = ?1, updated_by = ?1",
        "INSERT INTO out.dependencies SELECT * FROM main.dependencies
         WHERE owner_id = ?1 AND before_id IN (SELECT id FROM out.tasks) AND after_id IN (SELECT id FROM out.tasks)",
        "INSERT INTO out.completions SELECT * FROM main.completions WHERE task_id IN (SELECT id FROM out.tasks WHERE owner_id = ?1)",
        "INSERT INTO out.ai_jobs SELECT * FROM main.ai_jobs WHERE owner_id = ?1 AND (folder_id IS NULL OR folder_id IN (SELECT id FROM out.folders))",
        // Uploaded by members too.
        "INSERT INTO out.files (id, owner_id, type, size, created_at) SELECT id, ?1, type, size, created_at FROM main.files
         WHERE id IN (SELECT id FROM main.attachments WHERE task_id IN (SELECT id FROM out.tasks) OR job_id IN (SELECT id FROM out.ai_jobs))",
        "INSERT INTO out.attachments SELECT * FROM main.attachments WHERE id IN (SELECT id FROM out.files WHERE owner_id = ?1)",
        "INSERT INTO out.ai_notes SELECT * FROM main.ai_notes WHERE user_id = ?1 AND (folder_id IS NULL OR folder_id IN (SELECT id FROM out.folders))",
    ] {
        tx.execute(sql, [user])?;
    }
    let username = tx.query_row("SELECT username FROM out.users", [], |r| r.get(0))?;
    tx.commit()?;
    Ok(username)
}

/// What `POST /import` is given: the change's id, and the backup's files, uploaded.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ImportBackup {
    id: MutationId,
    /// The backup's database.
    database: AttachmentId,
    /// Each of the backup's attachments' files, by its id there.
    files: HashMap<AttachmentId, AttachmentId>,
}

/// How much an import added, and the batch it is, to undo.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct Imported {
    tasks: usize,
    folders: usize,
    batch: i64,
}

/// Adds a backup to the user's data (docs/local/backup.md#导入).
pub async fn import(State(st): State<AppState>, auth: Auth, Json(req): Json<ImportBackup>) -> Result<Json<Imported>, ApiError> {
    auth.require_write()?;
    if st.local.is_some() {
        return Err(ApiError::not_found("接口不存在"));
    }
    let ImportBackup { id, database, files } = req;
    let asked = auth.clone();
    let (made, upload) = st
        .db
        .call(move |c| {
            let actor = Actor { user: &asked.user_id, token_id: &asked.token_id, via: asked.via() };
            // The reply was lost, say: nothing to read again.
            let made = mutate::earlier(c, &actor, id)?.map(|applied| imported(&applied));
            Ok((made, store::load_file(c, database)?.filter(|f| f.owner == asked.user_id)))
        })
        .await?;
    if let Some(made) = made {
        return Ok(Json(made));
    }
    let size = upload.ok_or_else(|| ApiError::invalid("备份的数据库没有上传", Some("database".into())))?.size;
    let stored = st.files.clone();
    let import = tokio::task::spawn_blocking(move || {
        // Read on a copy: a file stays as it was uploaded.
        let copy = stored.copy(database, size)?;
        let (data, notes) = read(copy.path())?;
        remap(data, notes, &files)
    })
    .await
    .expect("reading the backup panicked")?;
    let (hub, now) = (st.hub.clone(), st.clock.now());
    let made = st
        .db
        .call(move |c| {
            let actor = Actor { user: &auth.user_id, token_id: &auth.token_id, via: auth.via() };
            let (applied, to) = mutate::import(c, &actor, id, import, now)?;
            hub.notify(&to);
            Ok(imported(&applied))
        })
        .await?;
    tracing::info!(tasks = made.tasks, folders = made.folders, "backup imported");
    Ok(Json(made))
}

fn imported(applied: &Applied) -> Imported {
    let created = |f: fn(&Effect) -> bool| applied.effects.iter().filter(|e| f(e)).count();
    Imported {
        tasks: created(|e| matches!(e, Effect::Task(Change::Created(_)))),
        folders: created(|e| matches!(e, Effect::Folder(Change::Created(_)))),
        batch: applied.seq,
    }
}

/// The data in the backup's database at `path`, checked as a restore checks it, then migrated.
fn read(path: &Path) -> Result<(Snapshot, HashMap<Option<FolderId>, NoteText>), ApiError> {
    let mut c = Connection::open(path)?;
    match db::ours(&c) {
        Ok(true) => {}
        Ok(false) => return Err(not_ours()),
        Err(e) if e.is::<db::Newer>() => return Err(ApiError::invalid("备份来自更新版本的 NextTodo，请先更新服务端", None)),
        Err(e) => return Err(failed(e)),
    }
    let users: Vec<String> = c.prepare("SELECT id FROM users")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let [user] = users.as_slice() else {
        return Err(ApiError::invalid(if users.is_empty() { "备份里没有数据" } else { "备份里有多个用户，不能导入" }, None));
    };
    c.pragma_update(None, "foreign_keys", true)?;
    db::migrate(&mut c).map_err(failed)?;
    // Not taken along; without them nothing reads down the folders before they are checked for circles.
    c.execute("DELETE FROM members", [])?;
    // Through JSON, as the API takes it: what it checks of each value is checked here too.
    let data = serde_json::from_value(serde_json::to_value(store::snapshot(&c, user)?)?).map_err(|_| not_ours())?;
    let notes = store::ai_notes(&c, user)?
        .into_iter()
        .map(|(folder, text)| Ok((folder, NoteText::try_from(String::from(text)).map_err(|_| not_ours())?)))
        .collect::<Result<_, ApiError>>()?;
    Ok((data, notes))
}

/// A backup's data as the account gets it: everything with a new id, made in the order of the old
/// ones so that what came first still does, and each attachment with the one its file was uploaded
/// as, as `files` has it.
fn remap(
    data: Snapshot,
    notes: HashMap<Option<FolderId>, NoteText>,
    files: &HashMap<AttachmentId, AttachmentId>,
) -> Result<Import, ApiError> {
    let Snapshot { folders, mut tasks, dependencies, completions, attachments, ai_jobs, .. } = data;
    let folder_ids = renew(folders.iter().map(|f| f.id), FolderId);
    let task_ids = renew(tasks.iter().map(|t| t.id), TaskId);
    let job_ids = renew(ai_jobs.iter().map(|j| j.id), AiJobId);
    let not_uploaded: Vec<&str> = attachments.iter().filter(|a| !files.contains_key(&a.id)).map(|a| a.name.as_str()).collect();
    if !not_uploaded.is_empty() {
        return Err(ApiError::not_uploaded(&not_uploaded));
    }
    if !acyclic(&dependencies.iter().map(|d| (d.before, d.after)).collect::<Vec<_>>()) {
        return Err(not_ours());
    }
    let links: Vec<(String, String)> =
        attachments.iter().map(|a| (format!("attachment:{}", a.id), format!("attachment:{}", files[&a.id]))).collect();
    // Numbered in the order they were.
    tasks.sort_by_key(|t| t.number);
    Ok(Import {
        folders: parents_first(folders)
            .ok_or_else(not_ours)?
            .into_iter()
            .map(|f| Folder { id: folder_ids[&f.id], parent_id: f.parent_id.map(|p| folder_ids[&p]), ..f })
            .collect(),
        tasks: tasks
            .into_iter()
            .map(|t| {
                let mut description = t.description.as_str().to_owned();
                if description.contains("attachment:") {
                    for (old, new) in &links {
                        description = description.replace(old, new);
                    }
                }
                Task {
                    id: task_ids[&t.id],
                    folder_id: t.folder_id.map(|f| folder_ids[&f]),
                    description: Description::try_from(description).expect("ids as long as before"),
                    assignees: Vec::new(),
                    ..t
                }
            })
            .collect(),
        dependencies: dependencies
            .into_iter()
            .map(|d| Dependency { before: task_ids[&d.before], after: task_ids[&d.after], ..d })
            .collect(),
        completions: completions.into_iter().map(|r| Completion { task_id: task_ids[&r.task_id], ..r }).collect(),
        // Not sent to this account's model unasked.
        jobs: ai_jobs
            .into_iter()
            .map(|j| AiJob {
                id: job_ids[&j.id],
                folder_id: j.folder_id.map(|f| folder_ids[&f]),
                state: JobState::Failed,
                error: Some("导入的请求，重试后整理".into()),
                suggestions: None,
                ..j
            })
            .collect(),
        attachments: attachments
            .into_iter()
            .map(|a| Attachment {
                id: files[&a.id],
                holder: match a.holder {
                    Holder::Task(t) => Holder::Task(task_ids[&t]),
                    Holder::Job(j) => Holder::Job(job_ids[&j]),
                },
                ..a
            })
            .collect(),
        notes: notes
            .into_iter()
            .filter_map(|(folder, text)| match folder {
                None => Some((None, text)),
                Some(f) => folder_ids.get(&f).map(|&f| (Some(f), text)),
            })
            .collect(),
    })
}

/// A new id for each of `old`, made in their order.
fn renew<T: Copy + Ord + Hash>(old: impl Iterator<Item = T>, new: fn(Uuid) -> T) -> HashMap<T, T> {
    let mut old: Vec<T> = old.collect();
    old.sort();
    old.into_iter().map(|id| (id, new(Uuid::now_v7()))).collect()
}

/// `folders`, each after the one it is in; none when some are in one another round in a circle.
fn parents_first(folders: Vec<Folder>) -> Option<Vec<Folder>> {
    let all = folders.len();
    let mut children: HashMap<Option<FolderId>, Vec<Folder>> = HashMap::new();
    for f in folders {
        children.entry(f.parent_id).or_default().push(f);
    }
    let mut ordered = Vec::with_capacity(all);
    let mut next = children.remove(&None).unwrap_or_default();
    while let Some(f) = next.pop() {
        next.extend(children.remove(&Some(f.id)).unwrap_or_default());
        ordered.push(f);
    }
    (ordered.len() == all).then_some(ordered)
}

fn not_ours() -> ApiError {
    ApiError::invalid(NOT_OURS, None)
}

/// What went wrong on the server's side, told as such.
fn failed(e: anyhow::Error) -> ApiError {
    tracing::error!(error = %e, "transfer failed");
    ApiError::internal()
}
