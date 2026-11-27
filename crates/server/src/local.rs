//! Local mode: the server inside the app, for this device alone, with one user who never signs in.
//! See docs/local.md.

use std::fs::{File, OpenOptions, TryLockError};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::extract::Request;
use axum::middleware::Next;
use rusqlite::{Connection, params};

use crate::ai::{Ai, llm::Http};
use crate::auth::{self, TokenKind};
use crate::backup::{self, DB_DIR, DB_FILE};
use crate::error::ApiError;
use crate::{AppState, app, db, start};

const USERNAME: &str = "local";
/// The local user's display name, and the name of the app's session.
const NAME: &str = "本机";

/// How long starting waits for an earlier process of the app to let go of the data directory.
const LOCK_WAIT: Duration = Duration::from_secs(10);
/// The header `answers` asks with.
const PROBE: &str = "nexttodo-probe";

#[derive(Debug)]
pub enum StartError {
    /// Another process of the app still has the data directory.
    Locked,
    Failed(anyhow::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Locked => f.write_str("NextTodo 的另一个进程还在运行"),
            StartError::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

impl StartError {
    /// What to tell the user (`error::for_user`).
    pub fn for_user(&self) -> String {
        match self {
            StartError::Locked => format!("启动本机模式失败：{self}"),
            StartError::Failed(e) => crate::error::for_user("启动本机模式", e.as_ref()),
        }
    }
}

impl From<anyhow::Error> for StartError {
    fn from(e: anyhow::Error) -> Self {
        StartError::Failed(e)
    }
}

impl From<std::io::Error> for StartError {
    fn from(e: std::io::Error) -> Self {
        StartError::Failed(e.into())
    }
}

/// The server running for this process, until it ends.
pub struct Local {
    /// Where the page reaches it: `http://127.0.0.1:<port>`.
    pub url: String,
    /// The local user's id.
    pub user: String,
    /// The database's epoch: see the `epoch` migration.
    pub epoch: String,
    state: AppState,
    /// The data directory.
    dir: PathBuf,
    /// The last `PROBE` the server heard.
    heard: Arc<Mutex<Option<String>>>,
    /// Held while the process runs; the system lets go of it when the process ends.
    _lock: File,
}

impl Local {
    /// Starts the server on the data directory `dir`, listening on 127.0.0.1 at a port the system
    /// picks.
    pub async fn start(dir: &Path) -> Result<Local, StartError> {
        Local::start_at(dir, 0).await
    }

    /// Starts the server listening at `port`: on iOS, where it started before it stopped, so that the
    /// page's address holds (docs/ios.md#本机模式). 0 is one the system picks.
    pub async fn start_at(dir: &Path, port: u16) -> Result<Local, StartError> {
        private_dir(dir)?;
        let lock = lock(&dir.join("lock"), LOCK_WAIT).await?;
        backup::finish(dir).context("无法完成恢复")?;
        std::fs::create_dir_all(dir.join(DB_DIR))?;
        let db = dir.join(DB_DIR).join(DB_FILE);
        let now = crate::Clock::system().now();
        let (started, (user, epoch)) = start::start(
            &db,
            dir.join("snapshots"),
            (Ipv4Addr::LOCALHOST, port).into(),
            Ai::new(None, Arc::new(Http { public_only: false })),
            Some(dir.to_owned()),
            move |c| Ok((local_user(c, now)?, db::epoch(c)?)),
        )
        .await?;
        let url = format!("http://{}", started.listener.local_addr()?);
        #[cfg(feature = "agent-port")]
        let agent_port = crate::agent_port::listen(dir).await?;
        let (state, router) = (started.state.clone(), app(started.state.clone()));
        // In the phone apps the page is at another origin (docs/local.md#页面直连).
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let router = router.layer(crate::page_cors());
        #[cfg(feature = "agent-port")]
        agent_port.serve(router.clone());
        let heard = Arc::new(Mutex::new(None));
        let hears = heard.clone();
        let router = router.layer(axum::middleware::from_fn(move |req: Request, next: Next| {
            if let Some(probe) = req.headers().get(PROBE).and_then(|v| v.to_str().ok()) {
                *hears.lock().unwrap_or_else(PoisonError::into_inner) = Some(probe.to_owned());
            }
            next.run(req)
        }));
        let serving = started.serve(router);
        tokio::spawn(async move {
            if let Err(e) = serving.await {
                tracing::error!("local server stopped: {e}");
            }
        });
        Ok(Local { url, user, epoch, state, dir: dir.to_owned(), heard, _lock: lock })
    }

    /// Whether this server still answers at its address: the system takes the port of an iOS app
    /// suspended a while, and another program may have it since, which would answer too, and get the
    /// token (docs/ios.md#本机模式). Asked with a value no other program knows, which this server
    /// tells it heard.
    pub async fn answers(&self) -> bool {
        let probe = auth::random_string(22);
        let (url, sent) = (format!("{}/api/v1/me", self.url), probe.clone());
        let asked = tokio::task::spawn_blocking(move || {
            let config = ureq::Agent::config_builder().proxy(None).timeout_global(Some(Duration::from_secs(2))).http_status_as_error(false);
            ureq::Agent::from(config.build()).get(url).header(PROBE, sent).call().map(drop)
        })
        .await
        .expect("asking panicked");
        asked.is_ok() && self.heard.lock().unwrap_or_else(PoisonError::into_inner).as_deref() == Some(probe.as_str())
    }

    /// Writes a backup of the data to `to`, replacing the file there; not in the data directory, where
    /// it could take the database's place.
    pub async fn backup(&self, to: PathBuf) -> anyhow::Result<()> {
        let (dir, files) = (self.dir.clone(), self.state.files.clone());
        tokio::task::spawn_blocking(move || {
            if to.parent().context("备份文件的路径无效")?.canonicalize()?.starts_with(dir.canonicalize()?) {
                anyhow::bail!("备份不能放在 NextTodo 的数据目录里");
            }
            backup::write(&dir, &files, &to)
        })
        .await?
    }

    /// Looks at the drive now, when syncing through one, waiting up to `wait` for what is new there to
    /// be read in (docs/local/drive.md#运行): as the app comes to the front, and before the widget lists the
    /// tasks.
    pub async fn pull(&self, wait: Duration) {
        if let Some(drive) = self.state.drive.current().await {
            drive.caught_up(wait).await;
        }
    }

    /// Whether it syncs through a drive: what the app's own part sends then needs the network.
    pub async fn syncing(&self) -> bool {
        self.state.drive.current().await.is_some()
    }

    /// Signs the app in: a new session for this device, the earlier ones revoked with it.
    pub async fn session(&self) -> Result<String, ApiError> {
        let (user, now, hub) = (self.user.clone(), self.state.clock.now(), self.state.hub.clone());
        self.state
            .db
            .call(move |c| {
                let tx = c.transaction()?;
                tx.execute("DELETE FROM tokens WHERE user_id = ?1 AND kind = ?2", params![user, TokenKind::Session])?;
                let secret = auth::create_session(&tx, &user, NAME, now)?;
                tx.commit()?;
                // Sync connections of the earlier sessions check their token again, and close.
                hub.changed(&user);
                Ok(secret)
            })
            .await
    }
}

/// Makes `dir` and the directories above it that are missing, writable by the user alone: the
/// address file in it says where agents send their token, and the data goes in and out by renames.
pub(crate) fn private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Takes the lock on the data directory, waiting up to `wait` for another process to let go of it.
async fn lock(path: &Path, wait: Duration) -> Result<File, StartError> {
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    let since = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if since.elapsed() < wait => tokio::time::sleep(Duration::from_millis(20)).await,
            Err(TryLockError::WouldBlock) => return Err(StartError::Locked),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
}

/// Makes the database the local user's (docs/local.md#本机用户), and returns their id.
pub(crate) fn local_user(c: &mut Connection, now: i64) -> Result<String, ApiError> {
    let tx = c.transaction()?;
    let users: Vec<String> = tx.prepare("SELECT id FROM users")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let id = match users.as_slice() {
        [] => auth::create_user(&tx, USERNAME, NAME, "", false, now)?,
        [id] => id.clone(),
        _ => return Err(ApiError::conflict("数据库里有多个用户，不能在本机使用")),
    };
    // No password verifies against an empty hash: no one signs in, and nothing needs a password.
    tx.execute(
        "UPDATE users SET username = ?1, display_name = ?2, password_hash = '', is_admin = 0 WHERE id = ?3",
        params![USERNAME, NAME, id],
    )?;
    tx.execute("DELETE FROM invites", [])?;
    // Sealed with a server's key, which the app doesn't have.
    tx.execute("DELETE FROM ai_settings", [])?;
    tx.commit()?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_lock_waits_for_the_process_holding_it() {
        let path = std::env::temp_dir().join(format!("nexttodo-lock-{}", uuid::Uuid::now_v7()));
        let held = lock(&path, Duration::ZERO).await.unwrap();
        assert!(matches!(lock(&path, Duration::from_millis(50)).await, Err(StartError::Locked)));
        let waiting = tokio::spawn({
            let path = path.clone();
            async move { lock(&path, Duration::from_secs(10)).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(held);
        assert!(waiting.await.unwrap().is_ok());
        std::fs::remove_file(path).unwrap();
    }
}
