//! SQLite access: one connection behind a mutex, used from blocking threads. The load is a handful of
//! users, so a single connection is simpler than a pool and never waits long.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::error::ApiError;

pub(crate) const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_ai.sql"),
    include_str!("../migrations/003_custom_ecnu.sql"),
    include_str!("../migrations/004_epoch.sql"),
    include_str!("../migrations/005_tidy.sql"),
    include_str!("../migrations/006_repeat.sql"),
    include_str!("../migrations/007_repeat_days.sql"),
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

/// Whether the database `c` is NextTodo's and intact, of this version or an earlier one: its tables
/// as that version's migrations make them, and nothing else. An error when it is from a newer one.
pub(crate) fn ours(c: &Connection) -> anyhow::Result<bool> {
    let Ok(check) = c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)) else { return Ok(false) };
    if check != "ok" || c.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Ok(false);
    }
    let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let done = MIGRATIONS.len() - pending(version)?.len();
    let empty = Connection::open_in_memory()?;
    for sql in &MIGRATIONS[..done] {
        empty.execute_batch(sql)?;
    }
    Ok(done > 0 && schema(c)? == schema(&empty)?)
}

/// The last sequence number given to a mutation.
fn last_seq(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT coalesce(max(seq), 0) FROM sqlite_sequence WHERE name = 'mutations'", [], |r| r.get(0))
}

fn schema(c: &Connection) -> rusqlite::Result<Vec<(String, String, Option<String>)>> {
    c.prepare("SELECT type, name, sql FROM sqlite_schema ORDER BY type, name")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect()
}

/// Replaces the database at `db`, with the server stopped, by a copy of NextTodo's database at
/// `from`, a snapshot say. The data replaced, damaged even, is kept as it was in
/// `before-restore-<Unix seconds>/`, returned when there was some.
///
/// Devices are to download everything again, and lists kept with a sequence number, as the
/// widget's, to take the new ones: the copy's log goes on above every number given before, every
/// user's cut (see `prune`) there. Those are the database replaced's; when it is damaged or gone,
/// `now_ms` stands for them: they went up a change at a time, from 1 or from an earlier restore's
/// time, so all are below it, unless the clock has gone back to before that restore, or the changes
/// since came faster than one a millisecond. Then devices that had seen the higher ones go on with
/// what they have.
pub fn restore(db: &Path, from: &Path, now_ms: i64) -> anyhow::Result<Option<PathBuf>> {
    // Read-only: a mistyped path is not made into an empty database.
    let source =
        Connection::open_with_flags(from, OpenFlags::SQLITE_OPEN_READ_ONLY).with_context(|| format!("无法打开 {}", from.display()))?;
    if !ours(&source)? {
        bail!("这不是 NextTodo 的数据库，或者文件已损坏");
    }
    let part = db.with_extension("db.restore");
    // Left by a restore cut short: VACUUM INTO won't write over it.
    if part.try_exists()? {
        std::fs::remove_file(&part)?;
    }
    source.execute("VACUUM INTO ?1", [part.to_str().expect("data directory path is UTF-8")])?;
    // From the database replaced itself, none of its files is open from here on.
    drop(source);
    let mut c = Connection::open(&part)?;
    migrate(&mut c)?;

    let files = ["db", "db-wal", "db-shm"].map(|suffix| db.with_extension(suffix));
    let mut there = Vec::new();
    for file in &files {
        if file.try_exists()? {
            there.push(file);
        }
    }
    let kept = if there.is_empty() {
        None
    } else {
        let kept = crate::backup::kept(db.parent().expect("a database in a directory"), now_ms)?;
        std::fs::create_dir(&kept)?;
        for file in there {
            std::fs::copy(file, kept.join(file.file_name().expect("a file name")))?;
        }
        Some(kept)
    };
    // Read once kept, as reading changes it.
    let given = if db.try_exists()? { replaced_seq(db)? } else { 0 };

    let tx = c.transaction()?;
    let cut = now_ms.max(given + 1).max(last_seq(&tx)? + 1);
    tx.execute("DELETE FROM sqlite_sequence WHERE name = 'mutations'", [])?;
    tx.execute("INSERT INTO sqlite_sequence (name, seq) VALUES ('mutations', ?1)", [cut])?;
    // Cleared up to the cut, as `prune` leaves it.
    tx.execute("UPDATE users SET pruned_seq = ?1", [cut])?;
    tx.execute("UPDATE mutations SET ops = NULL, effects = NULL, reply = NULL WHERE effects IS NOT NULL", [])?;
    tx.execute("UPDATE epoch SET value = lower(hex(randomblob(16)))", [])?;
    tx.commit()?;
    // One file, without a -wal beside it.
    c.pragma_update_and_check(None, "journal_mode", "DELETE", |_| Ok(()))?;
    drop(c);
    // Beside the copy, SQLite would apply the replaced data's -wal to it.
    for file in &files[1..] {
        if file.try_exists()? {
            std::fs::remove_file(file)?;
        }
    }
    std::fs::rename(&part, db)?;
    Ok(kept)
}

/// The last sequence number the database replaced gave, with its -wal taken in, so that, a restore
/// cut short, it is whole without it; 0 when it is damaged, or has no log, as an empty file. An error
/// when it is in use, or can't be read for another reason, the permissions or the disk.
fn replaced_seq(db: &Path) -> anyhow::Result<i64> {
    let read = Connection::open(db).and_then(|c| {
        let seq = last_seq(&c)?;
        Ok((seq, c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get::<_, i64>(0))?))
    });
    match read {
        Ok((seq, 0)) => Ok(seq),
        // A reader in another program: its -wal would go with frames not taken in.
        Ok(_) => bail!("现在的数据库正被别的程序使用"),
        // Unknown is SQLITE_ERROR: here, no table to read.
        Err(e) if matches!(e.sqlite_error_code(), Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase | ErrorCode::Unknown)) => {
            tracing::warn!(error = %e, "database replaced damaged, numbered on from the time");
            Ok(0)
        }
        Err(e) => Err(e).context("无法读取现在的数据库"),
    }
}
