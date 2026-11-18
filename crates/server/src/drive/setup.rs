//! Turning syncing through a drive on and off in the settings (docs/local.md#开启, #停止), and
//! resuming it as the app starts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use super::commit;
use super::remote::{Created, Failed, Remote};
use super::s3::S3;
use super::webdav::WebDav;
use super::{Drive, Status, Trouble, read_in, snapshot, snapshots};
use crate::auth::Auth;
use crate::error::{ApiError, Json};
use crate::{AppState, Db, backup, store};

/// Which generation of the drive's folder is the one, written last when it is made.
pub(crate) const POINTER: &str = "nexttodo.json";
/// The layout of the folder this version of the app writes, and the newest it reads
/// (docs/local.md#格式版本).
const FORMAT: i64 = 1;
/// Where the drive's settings are kept, in the data directory: not in the database, so that
/// backups, snapshots and the drive never have them.
const CONFIG: &str = "sync.json";
/// How this version of the app keeps them, and the newest it reads (docs/local.md#格式版本).
const CONFIG_FORMAT: i64 = 1;

/// What `CONFIG` holds.
#[derive(Serialize, Deserialize)]
struct Kept {
    format: i64,
    drive: Config,
}

/// Just the format of a file, read first: in a newer one, the rest may be anything.
#[derive(Deserialize)]
struct Format {
    format: i64,
}

#[derive(Deserialize)]
pub(crate) struct Pointer {
    pub id: String,
}

impl Pointer {
    pub(super) fn read(bytes: &[u8]) -> Result<Pointer, Trouble> {
        let not_ours = |_| Trouble::Unreadable("网盘的文件夹里有不是 NextTodo 写的 nexttodo.json".into());
        if serde_json::from_slice::<Format>(bytes).map_err(not_ours)?.format > FORMAT {
            return Err(Trouble::NewerVersion);
        }
        serde_json::from_slice(bytes).map_err(not_ours)
    }
}

/// `nexttodo.json` pointing to generation `id`.
pub(crate) fn pointer(id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "format": FORMAT, "id": id })).expect("JSON")
}

/// How to reach the drive, as the user set it up.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
#[ts(export, rename = "DriveConfig")]
pub enum Config {
    Webdav {
        url: String,
        user: String,
        password: String,
        folder: String,
    },
    S3 {
        endpoint: String,
        region: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        /// The bucket in the path, as self-hosted stores such as MinIO have it, rather than in the
        /// host.
        path_style: bool,
        folder: String,
    },
}

/// The drive's settings as shown: without the password or key.
#[derive(Serialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[ts(export, rename = "DriveConfigView")]
pub enum View {
    Webdav { url: String, user: String, folder: String },
    S3 { endpoint: String, bucket: String, folder: String },
}

impl Config {
    fn remote(&self) -> Result<Arc<dyn Remote>, ApiError> {
        let invalid = |m: &str, field: &str| ApiError::invalid(m, Some(field.into()));
        let folder = match self {
            Config::Webdav { folder, .. } | Config::S3 { folder, .. } => folder,
        };
        if folder.split('/').all(str::is_empty) || folder.split('/').any(|p| p == "." || p == "..") {
            return Err(invalid("请填写网盘里的文件夹", "folder"));
        }
        Ok(match self {
            Config::Webdav { url, user, password, folder } => {
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err(invalid("地址要以 https:// 或 http:// 开头", "url"));
                }
                Arc::new(WebDav::new(url.trim(), user.trim(), password, folder.trim()))
            }
            Config::S3 { endpoint, region, bucket, access_key, secret_key, path_style, folder } => {
                if bucket.trim().is_empty() {
                    return Err(invalid("请填写存储桶", "bucket"));
                }
                let region = if region.trim().is_empty() { "us-east-1" } else { region.trim() };
                Arc::new(
                    S3::new(endpoint.trim(), region, bucket.trim(), access_key.trim(), secret_key.trim(), *path_style, folder.trim())
                        .map_err(|m| invalid(&m, "endpoint"))?,
                )
            }
        })
    }

    fn view(&self) -> View {
        match self.clone() {
            Config::Webdav { url, user, folder, .. } => View::Webdav { url, user, folder },
            Config::S3 { endpoint, bucket, folder, .. } => View::S3 { endpoint, bucket, folder },
        }
    }

    /// As kept; one a newer version of the app kept is not read, and stays as it is.
    fn load(dir: &Path) -> anyhow::Result<Option<Config>> {
        let bytes = match std::fs::read(dir.join(CONFIG)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if serde_json::from_slice::<Format>(&bytes)?.format > CONFIG_FORMAT {
            return Err(crate::db::Newer.into());
        }
        Ok(Some(serde_json::from_slice::<Kept>(&bytes)?.drive))
    }

    /// Keeps it in `dir`, readable by the user alone, in place of what was there in one step.
    fn save(&self, dir: &Path) -> std::io::Result<()> {
        let part = dir.join(format!("{CONFIG}.part"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&part)?;
        let kept = Kept { format: CONFIG_FORMAT, drive: self.clone() };
        std::io::Write::write_all(&mut file, &serde_json::to_vec(&kept).expect("JSON"))?;
        file.sync_all()?;
        std::fs::rename(&part, dir.join(CONFIG))
    }
}

/// Syncing as it is, for the settings.
#[derive(Serialize, TS)]
#[ts(export, rename = "DriveSync")]
pub struct Shown {
    /// The drive it syncs through, while it does.
    drive: Option<View>,
    status: Option<Status>,
    /// Why syncing stopped by itself, if it did since the app started.
    stopped: Option<String>,
}

pub async fn get(State(st): State<AppState>, auth: Auth) -> Result<Json<Shown>, ApiError> {
    auth.require_device()?;
    let dir = data_dir(&st)?;
    let status = st.drive.current().await.map(|d| d.status());
    let drive = if status.is_some() { load(&dir)?.map(|c| c.view()) } else { None };
    Ok(Json(Shown { drive, status, stopped: st.drive.stopped() }))
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "DriveTurnOn")]
pub struct TurnOn {
    config: Config,
    /// When the folder has data: replace this device's with it. Without, that is asked first.
    #[serde(default)]
    replace: bool,
}

/// Turns syncing on: on an empty folder with this device's data, which goes up as the sync task's
/// first job; on one with data, by replacing this device's with it as a restore does, for the app to
/// start again with.
pub async fn put(State(st): State<AppState>, auth: Auth, Json(on): Json<TurnOn>) -> Result<StatusCode, ApiError> {
    auth.require_device()?;
    let dir = data_dir(&st)?;
    let remote = on.config.remote()?;
    turn_on(&st, &dir, on.config, remote, on.replace).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `put`, with `remote` the drive `config` is.
pub(super) async fn turn_on(st: &AppState, dir: &Path, config: Config, remote: Arc<dyn Remote>, replace: bool) -> Result<(), ApiError> {
    let _setting = st.drive.setting.lock().await;
    if st.drive.current().await.is_some() {
        return Err(ApiError::conflict("借网盘同步已经开着，请先停止"));
    }
    let probed = remote.clone();
    let pointer = tokio::task::spawn_blocking(move || probe(probed.as_ref())).await.expect("probing panicked")?;
    match pointer {
        None => {
            config.save(dir)?;
            Drive::enable(st, dir.to_owned(), remote).await.map_err(internal)
        }
        Some(_) if !replace => Err(ApiError::drive_has_data()),
        Some(pointer) => join(dir, remote, pointer, config).await,
    }
}

#[derive(Deserialize)]
pub struct Stop {
    /// Stop even when files of attachments in use can't be downloaded now.
    #[serde(default)]
    force: bool,
}

/// Stops syncing: the data here stays, with the files of the attachments in use downloaded first,
/// and the drive's as it is (docs/local.md#停止). Until the sync task has ended and the drive is
/// forgotten, changes wait, so that none is made here as the task still writes, or kept here only
/// while the next start syncs again.
pub async fn delete(State(st): State<AppState>, auth: Auth, query: Result<Query<Stop>, QueryRejection>) -> Result<StatusCode, ApiError> {
    auth.require_device()?;
    let Query(Stop { force }) = query.map_err(|e| ApiError::invalid(e.body_text(), Some("force".into())))?;
    let dir = data_dir(&st)?;
    let _setting = st.drive.setting.lock().await;
    if let Some(drive) = st.drive.current().await {
        // Offline, say, it says so before anything is stopped.
        if !force {
            download(&st).await?;
        }
        crate::ai::end_rounds(&st, &drive).await;
        drive.stop().await;
        // Those read in meanwhile: nothing changes which are in use now.
        if !force && let Err(e) = download(&st).await {
            // Its rounds ended as it stopped, and their failures may not be in: those, and not the
            // rounds begun once it syncs again.
            let ended = crate::ai::own_rounds(&st).await?;
            // Syncs on as before, through the same drive.
            Drive::start(&st, dir.clone(), drive.remote.clone()).await.map_err(internal)?;
            if let Some(drive) = st.drive.current().await {
                crate::ai::cut_rounds(&st, &drive, ended, crate::ai::STOPPED);
            }
            return Err(e);
        }
    }
    forget(&st.db, &dir).await?;
    st.files.set_origin(None);
    st.drive.turn_off(None).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Brings here the files still of use that are only in the drive yet: those of deleted attachments
/// too, which undoing brings back.
async fn download(st: &AppState) -> Result<(), ApiError> {
    let kept = st.db.call(|c| Ok(store::kept_files(c)?)).await?;
    let files = st.files.clone();
    let fetched = tokio::task::spawn_blocking(move || kept.into_iter().try_for_each(|(id, size)| files.fetch(id, size)))
        .await
        .expect("fetching panicked");
    fetched.map_err(|e| {
        tracing::warn!("downloading the files in use before syncing stops: {e}");
        ApiError::files_not_here()
    })
}

/// Syncs again as the app did when it last ran, if it did: before any request, so that every change
/// goes through the drive.
pub(crate) async fn resume(st: &AppState, dir: &Path) -> anyhow::Result<()> {
    let Some(config) = Config::load(dir)? else { return Ok(()) };
    let synced: bool = st.db.call(|c| Ok(c.query_row("SELECT EXISTS (SELECT 1 FROM sync_state)", [], |r| r.get(0))?)).await?;
    if !synced {
        // Turned on, and stopped before its first generation was made; or forgotten, and stopped
        // before the settings went.
        return Ok(forget(&st.db, dir).await?);
    }
    Drive::start(st, dir.to_owned(), config.remote()?).await
}

/// Forgets the drive: this device's place in its history, then its settings, which `resume` goes on
/// with should it stop in between.
pub(crate) async fn forget(db: &Db, dir: &Path) -> Result<(), ApiError> {
    db.call(|c| Ok(c.execute_batch("DELETE FROM sync_state; DELETE FROM sync_runs;")?)).await?;
    match std::fs::remove_file(dir.join(CONFIG)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Whether syncing through a drive is set up in data directory `dir`.
pub(crate) fn configured(dir: &Path) -> bool {
    dir.join(CONFIG).exists()
}

fn load(dir: &Path) -> Result<Option<Config>, ApiError> {
    Config::load(dir).map_err(internal)
}

fn data_dir(st: &AppState) -> Result<PathBuf, ApiError> {
    st.local.clone().ok_or_else(|| ApiError::not_found("接口不存在"))
}

fn internal(e: anyhow::Error) -> ApiError {
    tracing::error!("drive setup: {e:#}");
    ApiError::internal()
}

fn failed(f: Failed) -> ApiError {
    ApiError::unavailable(f.message())
}

/// Checks that `remote` can be synced through, writing where nothing is only (docs/local.md#开启),
/// and reads what `nexttodo.json` says, if it is there. Without it, the folder is to be NextTodo's
/// alone: what is cleared there later goes by name.
pub(super) fn probe(r: &dyn Remote) -> Result<Option<Pointer>, ApiError> {
    r.mkdir("").map_err(failed)?;
    r.mkdir("tmp").map_err(failed)?;
    let path = format!("tmp/{}", Uuid::now_v7());
    let written = (r.create(&path, b"1").map_err(failed)?, r.create(&path, b"2").map_err(failed)?);
    // What is left is cleared with `tmp/`.
    let _ = r.delete(&path);
    if written != (Created::Written, Created::Exists) {
        return Err(ApiError::invalid("这个网盘不能用来同步：同时写入时会互相覆盖", None));
    }
    let Some(bytes) = r.get(POINTER).map_err(failed)? else {
        // What NextTodo leaves in a folder of its own: `tmp/`, and generations made halfway.
        if r.list("").map_err(failed)?.iter().any(|name| name != "tmp" && !super::generation(name)) {
            return Err(ApiError::invalid("这个文件夹里有别的文件，请换一个空文件夹或新的文件夹名", Some("folder".into())));
        }
        return Ok(None);
    };
    Pointer::read(&bytes).map(Some).map_err(|t| ApiError::invalid(t.message(), None))
}

/// Prepares the drive's data to replace this device's, as a restore does: its latest snapshot, with
/// the commits after it read in, made the local user's with a new epoch. The app starts again with
/// it, and syncs.
async fn join(dir: &Path, remote: Arc<dyn Remote>, pointer: Pointer, config: Config) -> Result<(), ApiError> {
    let generation = pointer.id;
    let g = generation.clone();
    let (number, snapshot, commits) = tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
        let incomplete = || ApiError::invalid("网盘里的数据不完整：没有快照", None);
        let listed = remote.list(&format!("{g}/snapshots")).map_err(failed)?;
        let number = *snapshots(&listed).last().ok_or_else(incomplete)?;
        let snapshot = remote.get(&format!("{g}/snapshots/{number}.db.gz")).map_err(failed)?.ok_or_else(incomplete)?;
        let mut commits = Vec::new();
        while let Some(body) = remote.get(&commit::path(&g, number + 1 + i64::try_from(commits.len()).expect("few"))).map_err(failed)? {
            commits.push(body);
        }
        Ok((number, snapshot, commits))
    })
    .await
    .expect("downloading panicked")?;
    let (to, read) = (dir.to_owned(), commits.len());
    tokio::task::spawn_blocking(move || {
        backup::prepared(&to, |part| {
            let copy = part.join(backup::DB_FILE);
            snapshot::unpack(&snapshot, &copy)?;
            backup::convert(&copy, None)?;
            let mut c = Connection::open(&copy)?;
            c.pragma_update(None, "foreign_keys", true)?;
            let user: String = c.query_row("SELECT id FROM users", [], |r| r.get(0))?;
            // As each change was read in here, for the local user, and the log cleared up to where it
            // was: the snapshot has neither.
            c.execute_batch(
                "INSERT INTO deliveries (user_id, seq, effects) SELECT owner_id, seq, NULL FROM mutations WHERE effects IS NOT NULL;
                 UPDATE users SET pruned_seq = (SELECT coalesce(max(seq), 0) FROM mutations WHERE effects IS NULL);",
            )?;
            c.execute("INSERT INTO sync_state (id, generation, applied) VALUES (1, ?1, ?2)", params![generation, number])?;
            for (n, body) in (number + 1..).zip(&commits) {
                let commit = super::parse(body).map_err(|t| anyhow::anyhow!(t.message()))?;
                read_in(&mut c, &user, commit, n, &HashMap::new(), &HashMap::new())?;
            }
            Ok(())
        })?;
        config.save(&to)?;
        backup::commit(&to)
    })
    .await
    .expect("joining panicked")
    .map_err(|e| ApiError::invalid(crate::error::for_user("加入同步", e.as_ref()), None))?;
    tracing::info!(snapshot = number, commits = read, "joined a drive");
    Ok(())
}
