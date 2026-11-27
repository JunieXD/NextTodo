//! Attachments' files over HTTP, the server's settings, and clearing away files no longer used
//! (docs/api.md#附件, docs/data-model.md).

use std::collections::HashSet;
use std::io::ErrorKind;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use nexttodo_core::model::{Attachment, AttachmentId, FileSize, Holder, MediaType};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use ts_rs::TS;
use uuid::Uuid;

use crate::auth::Auth;
use crate::drive::Route;
use crate::error::{ApiError, Json};
use crate::files::{Files, Part, ReceiveError};
use crate::store::{self, StoredFile};
use crate::{AppState, MUTATION_RETENTION_MS};

/// How long a file no attachment uses is kept: as long as a change that deleted its attachment can
/// be undone.
const UNUSED_KEPT_MS: i64 = MUTATION_RETENTION_MS;
/// What an upload cut short or a server stopped left behind is cleared after this, well past any
/// upload still going on.
const STRAY_AGE: Duration = Duration::from_secs(24 * 3600);

/// The images agents and models are shown, rather than told of (docs/ai.md#附件).
pub const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
pub const MAX_IMAGE_SIZE: u32 = 5 * 1024 * 1024;

/// Whether `a` is an image that can be shown as is.
pub fn showable_image(a: &Attachment) -> bool {
    IMAGE_TYPES.contains(&a.media_type.as_str()) && a.size.get() <= MAX_IMAGE_SIZE
}

fn file_id(id: &str) -> Option<AttachmentId> {
    Uuid::parse_str(id).ok().and_then(|u| AttachmentId::try_from(u).ok())
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct Uploaded {
    id: AttachmentId,
    size: u32,
}

/// What an upload of `id` gets before it is taken in: what the first got, when it was the same
/// user's; nothing when the id is new.
fn uploaded_before(c: &Connection, user: &str, id: AttachmentId) -> Result<Option<Uploaded>, ApiError> {
    match store::load_file(c, id)? {
        Some(f) if f.owner == user => Ok(Some(Uploaded { id, size: f.size.get() })),
        Some(_) => Err(ApiError::conflict("附件 ID 已被占用")),
        // Used again, its file could still go from the drive, deleted late by the device that cleared it
        // (docs/local/drive.md#附件).
        None if store::file_purged(c, id)? => Err(ApiError::conflict("附件 ID 已被用过")),
        None => Ok(None),
    }
}

/// A copy of attachment `a`'s file, to be published as another id's: copying a folder copies its
/// attachments (docs/design.md#文件夹).
pub fn copy_file(files: &Files, a: &Attachment) -> Result<Part, ApiError> {
    files.copy(a.id, a.size).map_err(|e| match e.kind() {
        ErrorKind::NotFound => ApiError::not_found(format!("附件“{}”的文件不存在", a.name.as_str())),
        _ => e.into(),
    })
}

#[derive(Deserialize)]
pub struct Source {
    /// The attachment whose file this one is a copy of, rather than the body.
    pub from: Option<String>,
}

pub async fn put_file(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Query(source): Query<Source>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Uploaded>, ApiError> {
    auth.require_write()?;
    let id = file_id(&id).ok_or_else(|| ApiError::invalid("附件 ID 必须是 UUIDv7", Some("id".into())))?;
    let user = auth.user_id.clone();
    // Sent again after the reply was lost: no need to take it all in once more.
    if let Some(uploaded) = st.db.call(move |c| uploaded_before(c, &user, id)).await? {
        return Ok(Json(uploaded));
    }
    let (part, media_type) = match source.from {
        Some(from) => {
            let missing = || ApiError::not_found("附件不存在");
            let from = file_id(&from).ok_or_else(missing)?;
            let user = auth.user_id.clone();
            let a = st.db.call(move |c| Ok(visible(c, &user, from)?)).await?.ok_or_else(missing)?;
            let files = st.files.clone();
            let media_type = a.media_type.clone();
            (tokio::task::spawn_blocking(move || copy_file(&files, &a)).await.expect("copying panicked")?, media_type)
        }
        None => {
            let given = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream");
            let media_type = MediaType::try_from(given.split(';').next().unwrap_or_default().to_owned())
                .map_err(|m| ApiError::invalid(m, Some("Content-Type".into())))?;
            let part = st.files.receive(body).await.map_err(|e| match e {
                ReceiveError::TooLarge => ApiError::too_large(),
                ReceiveError::Body(_) => ApiError::invalid("上传中断", None),
                ReceiveError::Io(e) => e.into(),
            })?;
            (part, media_type)
        }
    };
    let size = FileSize::try_from(part.size).map_err(|m| ApiError::invalid(m, None))?;
    let route = st.drive.route().await;
    let (files, now, staged, user) = (st.files.clone(), st.clock.now(), matches!(route, Route::Drive(_)), auth.user_id);
    let uploaded = st
        .db
        .call(move |c| {
            // Checked again and published holding the database: two uploads of the same id can't
            // both get in, and the daily clearing can't take the file before it is recorded.
            let tx = c.transaction()?;
            if let Some(uploaded) = uploaded_before(&tx, &user, id)? {
                return Ok(uploaded);
            }
            store::insert_file(&tx, id, &StoredFile { owner: user, media_type, size }, now, staged)?;
            files.publish(part, id)?;
            tx.commit()?;
            Ok(Uploaded { id, size: size.get() })
        })
        .await?;
    tracing::info!(%id, size = uploaded.size, "file uploaded");
    Ok(Json(uploaded))
}

/// The attachment `id`, when `user` sees what it belongs to.
pub fn visible(c: &Connection, user: &str, id: AttachmentId) -> rusqlite::Result<Option<Attachment>> {
    let Some(a) = store::load_attachment(c, id)? else { return Ok(None) };
    let sees = match a.holder {
        Holder::Task(t) => store::sees_task(c, user, t)?,
        Holder::Job(j) => store::owns_job(c, user, j)?,
    };
    Ok(sees.then_some(a))
}

pub async fn get_file(State(st): State<AppState>, auth: Auth, Path(id): Path<String>, req: Request) -> Result<Response, ApiError> {
    let missing = || ApiError::not_found("附件不存在");
    let id = file_id(&id).ok_or_else(missing)?;
    let user = auth.user_id.clone();
    let (name, media_type, size) = st
        .db
        .call(move |c| {
            if let Some(a) = visible(c, &user, id)? {
                return Ok(Some((a.name.as_str().to_owned(), a.media_type, a.size)));
            }
            // Its uploader's until it is attached.
            let unattached = store::load_attachment(c, id)?.is_none();
            Ok(store::load_file(c, id)?.filter(|f| unattached && f.owner == user).map(|f| (id.to_string(), f.media_type, f.size)))
        })
        .await?
        .ok_or_else(missing)?;
    let files = st.files.clone();
    tokio::task::spawn_blocking(move || files.fetch(id, size)).await.expect("fetching panicked")?;
    let Ok(res) = st.files.serve(id).oneshot(req).await;
    if res.status() == StatusCode::NOT_FOUND {
        return Err(missing());
    }
    let mut res = res.map(Body::new);
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(media_type.as_str()).expect("media types are header text"));
    let disposition = format!("attachment; filename*=UTF-8''{}", encode(&name));
    h.insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition).expect("encoded to ASCII"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // An HTML or SVG file opened at this address runs nothing as the app.
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    // Who may read it, and under what name, can change: asked each time. The apps keep files themselves.
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// `s` percent-encoded as RFC 8187 has it, for `filename*`.
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The server's settings, for everyone on it.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ServerSettings {
    /// Whether photos are made smaller before they are uploaded (docs/design/attachments.md).
    pub compress_images: bool,
}

fn settings(c: &Connection) -> rusqlite::Result<ServerSettings> {
    c.query_row("SELECT compress_images FROM server_settings", [], |r| Ok(ServerSettings { compress_images: r.get(0)? }))
}

pub async fn get_settings(State(st): State<AppState>, _: Auth) -> Result<Json<ServerSettings>, ApiError> {
    st.db.call(|c| Ok(settings(c)?)).await.map(Json)
}

/// The administrator's, or in local mode the one user's.
pub async fn put_settings(State(st): State<AppState>, auth: Auth, Json(s): Json<ServerSettings>) -> Result<Json<ServerSettings>, ApiError> {
    auth.require_device()?;
    if !auth.is_admin && st.local.is_none() {
        return Err(ApiError::forbidden("仅管理员可以改服务器的设置"));
    }
    st.db
        .call(move |c| {
            c.execute("UPDATE server_settings SET compress_images = ?1", params![s.compress_images])?;
            Ok(settings(c)?)
        })
        .await
        .map(Json)
}

/// Files no attachment has used since before this are cleared: undoing can't need them any more.
pub fn unused_before(now: i64) -> i64 {
    now - UNUSED_KEPT_MS
}

/// Deletes the files no attachment has used for as long as undoing could need them, and what
/// uploads cut short and stops left behind. Holding the database all along, so that nothing is
/// attached, restored or published meanwhile. Returns how many of each went.
pub fn clear(c: &mut Connection, files: &Files, now: i64) -> Result<(usize, usize), ApiError> {
    let tx = c.transaction()?;
    let expired: Vec<AttachmentId> = tx
        .prepare("DELETE FROM files WHERE unused_at < ?1 RETURNING id")?
        .query_map([unused_before(now)], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    tx.commit()?;
    // Unrecorded now, nothing can bring them back: gone, or left for the sweep when this fails.
    for id in &expired {
        files.remove(*id)?;
    }
    Ok((expired.len(), sweep(c, files)?))
}

/// Deletes what uploads cut short and stops left behind, and files cleared through a drive that came
/// back meanwhile; how many.
pub fn sweep(c: &Connection, files: &Files) -> Result<usize, ApiError> {
    let known: HashSet<AttachmentId> =
        c.prepare("SELECT id FROM files WHERE NOT purged")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(files.sweep(&known, STRAY_AGE)?)
}
