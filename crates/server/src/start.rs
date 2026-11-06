//! Starting the server, the same way on a server and in the app (docs/local.md#启动): the database
//! checked, opened and migrated, then the jobs a stop cut short failed, then listening, and only then
//! anything in the background. A failure on the way leaves nothing running.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use rusqlite::Connection;
use tokio::net::TcpListener;

use crate::ai::{self, Ai};
use crate::error::ApiError;
use crate::{AppState, Clock, Db, MUTATION_RETENTION_MS, db};

/// How many daily snapshots of the database are kept.
const SNAPSHOTS_KEPT: usize = 7;

/// Ready to serve, with nothing running yet.
pub struct Started {
    pub state: AppState,
    pub listener: TcpListener,
    db: PathBuf,
    snapshots: PathBuf,
}

/// Opens the database at `db`, runs `prepare` on it, fails the jobs a stop cut short and listens on
/// `listen`. Daily snapshots will go to `snapshots`.
pub async fn start<T: Send + 'static>(
    db: &Path,
    snapshots: PathBuf,
    listen: SocketAddr,
    ai: Ai,
    prepare: impl FnOnce(&mut Connection) -> Result<T, ApiError> + Send + 'static,
) -> anyhow::Result<(Started, T)> {
    let state = AppState::new(Db::open(db)?, Clock::system()).with_ai(ai);
    let prepared = state.db.call(prepare).await?;
    // Before any request: a job the user starts would otherwise race these.
    ai::interrupted(&state).await?;
    let listener = TcpListener::bind(listen).await?;
    Ok((Started { state, listener, db: db.to_owned(), snapshots }, prepared))
}

impl Started {
    /// Starts the daily snapshots and the AI queue, and serves `router`.
    pub fn serve(self, router: Router) -> axum::serve::Serve<TcpListener, Router, Router> {
        tokio::spawn(daily(self.state.clone(), self.db, self.snapshots));
        tokio::spawn(ai::run_queue(self.state));
        axum::serve(self.listener, router)
    }
}

/// Once at start and then every day: clears the old part of the mutation log and snapshots the
/// database. A failure is logged and tried again the next day.
async fn daily(state: AppState, db: PathBuf, snapshots: PathBuf) {
    let mut tick = tokio::time::interval(Duration::from_secs(24 * 3600));
    loop {
        tick.tick().await;
        let cutoff = state.clock.now() - MUTATION_RETENTION_MS;
        // a failure is already logged by the error conversion
        if let Ok(cleared) = state.db.call(move |c| Ok(db::prune(c, cutoff)?)).await {
            tracing::info!(cleared, "pruned mutation log");
        }
        let (db, snapshots, now) = (db.clone(), snapshots.clone(), state.clock.now());
        match tokio::task::spawn_blocking(move || db::snapshot(&db, &snapshots, now, SNAPSHOTS_KEPT)).await.expect("snapshot task panicked")
        {
            Ok(path) => tracing::info!(path = %path.display(), "snapshot written"),
            Err(e) => tracing::error!("snapshot failed: {e:#}"),
        }
    }
}
