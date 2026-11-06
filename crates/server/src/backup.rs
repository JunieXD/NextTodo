//! Backing up and restoring local mode's data (docs/local.md#备份和恢复). Restoring needs no server
//! running, so that it works when starting failed too: a backup is prepared on a copy beside the
//! data, the user's go-ahead commits it, and the next start puts it in place.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use rusqlite::Connection;

use crate::{Clock, db, local};

/// A backup being prepared; not committed yet.
const PART: &str = "restore.part";
/// A backup committed, for the next start to put in place of `db/`.
const READY: &str = "restore";
pub(crate) const DB_DIR: &str = "db";
pub(crate) const DB_FILE: &str = "nexttodo.db";

/// Writes a consistent copy of the database at `db` to `to`, replacing what is there in one step.
pub(crate) fn write(db: &Path, to: &Path) -> anyhow::Result<()> {
    let mut partial = to.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    // Left by a backup cut short: VACUUM INTO won't write over it.
    gone(std::fs::remove_file(&partial))?;
    let name = partial.to_str().context("备份文件的路径无效")?;
    let written = Connection::open(db).and_then(|c| c.execute("VACUUM INTO ?1", [name])).map_err(anyhow::Error::from);
    let done = written.and_then(|_| Ok(std::fs::rename(&partial, to)?));
    if done.is_err() {
        // What failed is what to tell; this copy, if any, is incomplete.
        let _ = std::fs::remove_file(&partial);
    }
    done
}

/// Prepares the backup at `from` to replace the data in `dir`: on a copy, checked and made the local
/// user's, with a new epoch. When it won't do, the copy goes again and nothing else has changed.
pub fn prepare(dir: &Path, from: &Path) -> anyhow::Result<()> {
    let part = dir.join(PART);
    gone(std::fs::remove_dir_all(&part))?;
    local::private_dir(&part)?;
    let copy = part.join(DB_FILE);
    let done = std::fs::copy(from, &copy).context("无法读取备份文件").and_then(|_| convert(&copy));
    let done = done.and_then(|()| {
        // Writable: Windows flushes no file opened for reading only.
        std::fs::OpenOptions::new().write(true).open(&copy)?.sync_all()?;
        sync_dir(&part)?;
        Ok(sync_dir(dir)?)
    });
    if done.is_err() {
        // What failed is what to tell; a copy left behind goes at the next start.
        let _ = std::fs::remove_dir_all(&part);
    }
    done
}

/// Checks that the database at `copy` is NextTodo's, of this version or an earlier one, with one
/// user, then migrates it and makes it the local user's.
fn convert(copy: &Path) -> anyhow::Result<()> {
    let not_ours = || anyhow!("这不是 NextTodo 的备份，或者文件已损坏");
    let mut c = Connection::open(copy)?;
    let check: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0)).map_err(|_| not_ours())?;
    if check != "ok" || c.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err(not_ours());
    }
    let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let done = db::MIGRATIONS.len() - db::pending(version)?.len();
    // The tables as that version's migrations make them, and nothing else.
    let empty = Connection::open_in_memory()?;
    for sql in &db::MIGRATIONS[..done] {
        empty.execute_batch(sql)?;
    }
    if done == 0 || schema(&c)? != schema(&empty)? {
        return Err(not_ours());
    }
    match c.query_row("SELECT count(*) FROM users", [], |r| r.get::<_, i64>(0))? {
        1 => {}
        0 => bail!("备份里没有数据"),
        _ => bail!("备份里有多个用户，不能在本机使用"),
    }
    c.pragma_update(None, "foreign_keys", true)?;
    db::migrate(&mut c)?;
    local::local_user(&mut c, Clock::system().now())?;
    // Even a backup of this very device's: it is not the history the data has gone on with since.
    c.execute("UPDATE epoch SET value = lower(hex(randomblob(16)))", [])?;
    // One file, without a -wal beside it.
    c.pragma_update_and_check(None, "journal_mode", "DELETE", |_| Ok(()))?;
    Ok(())
}

fn schema(c: &Connection) -> rusqlite::Result<Vec<(String, String, Option<String>)>> {
    c.prepare("SELECT type, name, sql FROM sqlite_schema ORDER BY type, name")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect()
}

/// The user's go-ahead: from here on the backup replaces the data, at the next start.
pub fn commit(dir: &Path) -> anyhow::Result<()> {
    std::fs::rename(dir.join(PART), dir.join(READY))?;
    Ok(sync_dir(dir)?)
}

/// Carries on with a restore where it was left, before the database is opened. Each step is a
/// rename in `dir`, so that a start after any of them goes on from there. The data replaced is kept
/// in `before-restore-<Unix seconds>`.
pub(crate) fn finish(dir: &Path) -> anyhow::Result<()> {
    gone(std::fs::remove_dir_all(dir.join(PART)))?;
    let ready = dir.join(READY);
    if !ready.try_exists()? {
        return Ok(());
    }
    let data = dir.join(DB_DIR);
    if data.try_exists()? {
        let name = format!("before-restore-{}", Clock::system().now() / 1000);
        let mut kept = dir.join(&name);
        for n in 2.. {
            if !kept.try_exists()? {
                break;
            }
            kept = dir.join(format!("{name}-{n}"));
        }
        std::fs::rename(&data, kept)?;
        sync_dir(dir)?;
    }
    std::fs::rename(ready, data)?;
    Ok(sync_dir(dir)?)
}

/// A removal, where nothing to remove counts as done.
fn gone(removed: std::io::Result<()>) -> std::io::Result<()> {
    match removed {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

/// Writes a directory's entries to disk, after a rename in it.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Windows opens no directory for that without unsafe calls; NTFS's journal is what there is.
#[cfg(not(unix))]
fn sync_dir(_: &Path) -> std::io::Result<()> {
    Ok(())
}
