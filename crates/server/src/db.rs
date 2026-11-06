//! SQLite access: one connection behind a mutex, used from blocking threads. The load is a handful of
//! users, so a single connection is simpler than a pool and never waits long.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{anyhow, bail};
use rusqlite::{Connection, OpenFlags};

use crate::error::ApiError;

pub(crate) const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_ai.sql"),
    include_str!("../migrations/003_custom_ecnu.sql"),
    include_str!("../migrations/004_epoch.sql"),
];

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

/// The migrations a database at `version` still needs; an error when it is not one of ours, or from
/// a newer version of NextTodo.
pub(crate) fn pending(version: i64) -> anyhow::Result<&'static [&'static str]> {
    let Ok(done) = usize::try_from(version) else { bail!("数据库损坏") };
    match MIGRATIONS.get(done..) {
        Some(pending) => Ok(pending),
        None => bail!("数据来自更高版本的 NextTodo，请先更新"),
    }
}

impl Db {
    /// Opens the database at `path`, made when missing, and migrates it. One that would not migrate
    /// is looked at read-only first and left as it is.
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        if path.try_exists()? {
            let version = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .and_then(|c| c.pragma_query_value(None, "user_version", |r| r.get(0)))
                .map_err(|e| {
                    tracing::error!(error = %e, "database unreadable");
                    anyhow!("数据库损坏")
                })?;
            pending(version)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> anyhow::Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> anyhow::Result<Db> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        migrate(&mut conn)?;
        Ok(Db(Arc::new(Mutex::new(conn))))
    }

    /// Runs `f` with the connection on a blocking thread.
    pub async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        let conn = self.0.clone();
        // A panic inside `f` leaves no open transaction behind (it rolls back on drop), so the
        // connection stays usable even though the mutex is poisoned.
        tokio::task::spawn_blocking(move || f(&mut conn.lock().unwrap_or_else(PoisonError::into_inner)))
            .await
            .expect("database task panicked")
    }
}

/// Brings the database up to this version, all in one transaction.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let tx = conn.transaction()?;
    for sql in pending(version)? {
        tx.execute_batch(sql)?;
    }
    tx.pragma_update(None, "user_version", i64::try_from(MIGRATIONS.len())?)?;
    tx.commit()?;
    Ok(())
}

/// The name of the database's history: see the `epoch` migration.
pub fn epoch(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("SELECT value FROM epoch", [], |r| r.get(0))
}

/// Clears the operations and effects of mutations older than `cutoff`. The rows stay, so that a
/// resubmitted old mutation is recognised instead of applied again. Per user the log is cut at a
/// sequence number, which only moves forward: everything up to it is cleared, nothing after it.
pub fn prune(conn: &mut Connection, cutoff: i64) -> rusqlite::Result<usize> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE users SET pruned_seq = max(pruned_seq, coalesce(
             (SELECT max(seq) FROM mutations WHERE owner_id = users.id AND created_at < ?1 AND effects IS NOT NULL), 0))",
        [cutoff],
    )?;
    let cleared = tx.execute(
        "UPDATE mutations SET ops = NULL, effects = NULL, reply = NULL
         WHERE effects IS NOT NULL AND seq <= (SELECT pruned_seq FROM users WHERE id = mutations.owner_id)",
        [],
    )?;
    tx.commit()?;
    Ok(cleared)
}

/// Writes a consistent copy of the database at `db` to `dir/snapshot-<unix seconds>.db` and deletes
/// all but the newest `keep` copies. Uses a connection of its own, so requests don't wait for it. The
/// copy is written under a temporary name first, so that a crash never leaves a half-written
/// snapshot that looks complete. Other files in the directory are left alone.
pub fn snapshot(db: &Path, dir: &Path, now_ms: i64, keep: usize) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    // The files named like snapshots, oldest first.
    let files = |suffix: &str| -> std::io::Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let secs = name.to_str().and_then(|n| n.strip_prefix("snapshot-")?.strip_suffix(suffix));
            if secs.is_some_and(|s| s.len() == 10 && s.bytes().all(|b| b.is_ascii_digit())) && entry.file_type()?.is_file() {
                found.push(entry.path());
            }
        }
        found.sort();
        Ok(found)
    };
    // Left over from a crash: never complete, and VACUUM INTO refuses to overwrite.
    for p in files(".db.partial")? {
        std::fs::remove_file(p)?;
    }
    let path = dir.join(format!("snapshot-{:010}.db", now_ms / 1000));
    let partial = path.with_extension("db.partial");
    Connection::open(db)?.execute("VACUUM INTO ?1", [partial.to_str().expect("data directory path is UTF-8")])?;
    std::fs::rename(&partial, &path)?;
    let all = files(".db")?;
    for p in &all[..all.len().saturating_sub(keep)] {
        std::fs::remove_file(p)?;
    }
    Ok(path)
}
