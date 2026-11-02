//! SQLite access: one connection behind a mutex, used from blocking threads. The load is a handful of
//! users, so a single connection is simpler than a pool and never waits long.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rusqlite::Connection;

use crate::error::ApiError;

const MIGRATIONS: &[&str] = &[include_str!("../migrations/001_init.sql")];

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
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
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let tx = conn.transaction()?;
        for sql in &MIGRATIONS[usize::try_from(version)?..] {
            tx.execute_batch(sql)?;
        }
        tx.pragma_update(None, "user_version", i64::try_from(MIGRATIONS.len())?)?;
        tx.commit()?;
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
