//! Local mode: the server inside the app, for this device alone, with one user who never signs in.
//! See docs/local.md.

use std::fs::{File, OpenOptions, TryLockError};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use rusqlite::{Connection, params};

use crate::ai::{Ai, llm::Https};
use crate::auth::{self, TokenKind};
use crate::backup::{self, DB_DIR, DB_FILE};
use crate::error::ApiError;
use crate::{AppState, app, db, start};

const USERNAME: &str = "local";
/// The local user's display name, and the name of the app's session.
const NAME: &str = "本机";

/// How long starting waits for an earlier process of the app to let go of the data directory.
const LOCK_WAIT: Duration = Duration::from_secs(10);

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
    db: PathBuf,
    /// Held while the process runs; the system lets go of it when the process ends.
    _lock: File,
}

impl Local {
    /// Starts the server on the data directory `dir`, listening on 127.0.0.1 at a port the system
    /// picks.
    pub async fn start(dir: &Path) -> Result<Local, StartError> {
        private_dir(dir)?;
        let lock = lock(&dir.join("lock"), LOCK_WAIT).await?;
        backup::finish(dir).context("无法完成恢复")?;
        std::fs::create_dir_all(dir.join(DB_DIR))?;
        let db = dir.join(DB_DIR).join(DB_FILE);
        let now = crate::Clock::system().now();
        let (started, (user, epoch)) =
            start::start(&db, dir.join("snapshots"), (Ipv4Addr::LOCALHOST, 0).into(), Ai::new(None, Arc::new(Https)), move |c| {
                Ok((local_user(c, now)?, db::epoch(c)?))
            })
            .await?;
        let url = format!("http://{}", started.listener.local_addr()?);
        #[cfg(feature = "agent-port")]
        let agent_port = crate::agent_port::listen(dir).await?;
        let (state, router) = (started.state.clone(), app(started.state.clone()));
        #[cfg(target_os = "android")]
        let router = router.layer(page_cors());
        #[cfg(feature = "agent-port")]
        agent_port.serve(router.clone());
        let serving = started.serve(router);
        tokio::spawn(async move {
            if let Err(e) = serving.await {
                tracing::error!("local server stopped: {e}");
            }
        });
        Ok(Local { url, user, epoch, state, db, _lock: lock })
    }

    /// Writes a backup of the data to `to`, replacing the file there.
    pub async fn backup(&self, to: PathBuf) -> anyhow::Result<()> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || backup::write(&db, &to)).await?
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

/// On Android the page is at `https://localhost`, another origin. It sends the session's token in
/// `Authorization`; no cookie is ever set here (docs/local.md#页面直连).
#[cfg(target_os = "android")]
fn page_cors() -> tower_http::cors::CorsLayer {
    use axum::http::{HeaderValue, Method, header};
    tower_http::cors::CorsLayer::new()
        .allow_origin(HeaderValue::from_static("https://localhost"))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
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
