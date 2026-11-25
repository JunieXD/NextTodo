//! Backing up and restoring local mode's data (docs/local.md#备份和恢复). A backup is a tar of the
//! database and its attachments' files. Restoring needs no server running, so that it works when
//! starting failed too: a backup is prepared on a copy beside the data, the user's go-ahead commits
//! it, and the next start puts it in place.

use std::collections::HashMap;
use std::fs::File;
use std::io::{ErrorKind, Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use nexttodo_core::model::{AttachmentId, BACKUP_FORMAT, FileSize, MAX_FILE_SIZE};
use rusqlite::Connection;
use uuid::Uuid;

use crate::files::{self, Files};
use crate::{Clock, db, drive, local};

/// A backup being prepared; not committed yet.
const PART: &str = "restore.part";
/// A backup committed, for the next start to put in place of `db/`.
const READY: &str = "restore";
pub(crate) const DB_DIR: &str = "db";
pub(crate) const DB_FILE: &str = "nexttodo.db";
/// The copy of the database a backup takes, in the data directory until it is in the backup.
const COPY: &str = "backup.part";
/// What a backup is, first in it: `{"format": 1}`.
const MANIFEST: &str = "backup.json";

pub(crate) const NOT_OURS: &str = "这不是 NextTodo 的备份，或者文件已损坏";

/// Writes a backup of the data in `dir` to `to`, replacing what is there in one step: a tar of the
/// files a consistent copy of the database lists, `files/<id>`, and of the copy, `nexttodo.db`.
/// `files` are those of the data, brought from the drive when syncing through one.
pub(crate) fn write(dir: &Path, files: &Files, to: &Path) -> anyhow::Result<()> {
    let mut partial = to.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let copy = dir.join(COPY);
    // Left by a backup cut short: VACUUM INTO won't write over it.
    gone(std::fs::remove_file(&copy))?;
    let done = pack(&dir.join(DB_DIR), files, &copy, &partial).and_then(|()| Ok(std::fs::rename(&partial, to)?));
    // What failed is what to tell; the copy is of no more use, and a tar left is incomplete.
    let _ = std::fs::remove_file(&copy);
    if done.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    done
}

/// Writes to `to` a tar of the database in `data`, copied to `copy` first, and of its files.
fn pack(data: &Path, files: &Files, copy: &Path, to: &Path) -> anyhow::Result<()> {
    let name = copy.to_str().context("数据目录的路径无效")?;
    Connection::open(data.join(DB_FILE))?.execute("VACUUM INTO ?1", [name])?;
    tar(copy, files, to)
}

/// Writes to `to` a backup of the database at `copy`, which no one else has open: a tar of it and of
/// the files it lists, which `files` has. An export from a server is one too (docs/local.md#导出).
pub(crate) fn tar(copy: &Path, files: &Files, to: &Path) -> anyhow::Result<()> {
    // All not cleared: those in use, and those undoing a deletion brings back (docs/local.md#备份). In
    // the order they were added, which an import's new ids keep.
    let listed: Vec<(AttachmentId, FileSize, bool)> = Connection::open(copy)?
        .prepare("SELECT id, size, unused_at IS NOT NULL FROM files WHERE NOT purged ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut tar = tar::Builder::new(File::create(to)?);
    tar.mode(tar::HeaderMode::Deterministic);
    let manifest = serde_json::to_vec(&serde_json::json!({ "format": BACKUP_FORMAT })).expect("JSON");
    append(&mut tar, MANIFEST, &manifest)?;
    for (id, size, unused) in listed {
        // Syncing through a drive, another device's may not be here yet.
        files.fetch(id, size).context("部分附件的文件尚未从网盘下载，请稍后重试")?;
        match files.here(id).with_context(|| format!("无法读取附件的文件 {id}"))? {
            Some(bytes) => append(&mut tar, &format!("{}/{id}", files::DIR), &bytes)?,
            // Deleted by the daily clearing since the copy was taken: no one used it then already.
            None if unused => {}
            None => bail!("无法读取附件的文件 {id}"),
        }
    }
    // Last, so that a backup cut short at an entry's end, which reads as a whole tar, has none.
    tar.append_path_with_name(copy, DB_FILE)?;
    tar.into_inner()?.sync_all()?;
    Ok(())
}

fn append(tar: &mut tar::Builder<File>, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    tar.append_data(&mut header, name, bytes)
}

/// Prepares the backup at `from` to replace the data in `dir`: on a copy, checked and made the local
/// user's, with a new epoch. When it won't do, the copy goes again and nothing else has changed.
pub fn prepare(dir: &Path, from: &Path) -> anyhow::Result<()> {
    if drive::setup::configured(dir) {
        // Its data is not the history the drive's goes on with (docs/local.md#停止).
        bail!("借网盘同步开着时不能恢复备份，请先停止同步");
    }
    prepared(dir, |part| unpack(from, part).and_then(|files| convert(&part.join(DB_FILE), files)))
}

/// Prepares data to replace the data in `dir`, as `fill` makes it in the directory it is given: its
/// database, `DB_FILE`, checked and made the local user's, and its files. When it won't do, it goes
/// again and nothing else has changed.
pub(crate) fn prepared(dir: &Path, fill: impl FnOnce(&Path) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let part = dir.join(PART);
    gone(std::fs::remove_dir_all(&part))?;
    local::private_dir(&part)?;
    let copy = part.join(DB_FILE);
    let done = fill(&part).and_then(|()| {
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

/// Puts the backup at `from` in `part`: a tar's database and files, after its manifest and nothing
/// else in it, returning the files' sizes; or a database alone, as backups were before there were
/// attachments. One a newer version of the app wrote is `db::Newer`.
fn unpack(from: &Path, part: &Path) -> anyhow::Result<Option<HashMap<AttachmentId, u64>>> {
    let mut backup = File::open(from).context("无法读取备份文件")?;
    let mut head = [0; 16];
    if backup.read_exact(&mut head).is_ok() && &head == b"SQLite format 3\0" {
        std::fs::copy(from, part.join(DB_FILE)).context("无法读取备份文件")?;
        return Ok(None);
    }
    backup.rewind()?;
    let dir = part.join(files::DIR);
    std::fs::create_dir(&dir)?;
    let (mut database, mut sizes) = (false, HashMap::new());
    let mut archive = tar::Archive::new(backup);
    let mut entries = archive.entries().context(NOT_OURS)?;
    let mut manifest = entries.next().context(NOT_OURS)?.context(NOT_OURS)?;
    if manifest.path().context(NOT_OURS)?.to_str() != Some(MANIFEST) || manifest.size() > 1024 {
        bail!(NOT_OURS);
    }
    #[derive(serde::Deserialize)]
    struct Manifest {
        format: i64,
    }
    let mut bytes = Vec::new();
    manifest.read_to_end(&mut bytes)?;
    if serde_json::from_slice::<Manifest>(&bytes).context(NOT_OURS)?.format > BACKUP_FORMAT {
        return Err(db::Newer.into());
    }
    for entry in entries {
        let mut entry = entry.context(NOT_OURS)?;
        let name = entry.path().context(NOT_OURS)?.to_str().context(NOT_OURS)?.to_owned();
        let id = match name.strip_prefix(&format!("{}/", files::DIR)) {
            None if name == DB_FILE && !database => None,
            Some(id) if entry.size() <= u64::from(MAX_FILE_SIZE) => {
                let id = id.parse::<Uuid>().ok().and_then(|u| AttachmentId::try_from(u).ok());
                Some(id.filter(|id| !sizes.contains_key(id)).context(NOT_OURS)?)
            }
            _ => bail!(NOT_OURS),
        };
        if !entry.header().entry_type().is_file() {
            bail!(NOT_OURS);
        }
        let mut file = File::create(id.map_or_else(|| part.join(DB_FILE), |id| dir.join(id.to_string())))?;
        let size = std::io::copy(&mut entry, &mut file)?;
        file.sync_all()?;
        match id {
            Some(id) => _ = sizes.insert(id, size),
            None => database = true,
        }
    }
    if !database {
        bail!(NOT_OURS);
    }
    sync_dir(&dir)?;
    Ok(Some(sizes))
}

/// Checks that the database at `copy` is NextTodo's, of this version or an earlier one, with one
/// user, and that `files`, a tar's, are what it lists, then migrates it and makes it the local user's.
pub(crate) fn convert(copy: &Path, files: Option<HashMap<AttachmentId, u64>>) -> anyhow::Result<()> {
    let mut c = Connection::open(copy)?;
    if !db::ours(&c)? {
        bail!(NOT_OURS);
    }
    match c.query_row("SELECT count(*) FROM users", [], |r| r.get::<_, i64>(0))? {
        1 => {}
        0 => bail!("备份里没有数据"),
        _ => bail!("备份里有多个用户，不能在本机使用"),
    }
    c.pragma_update(None, "foreign_keys", true)?;
    db::migrate(&mut c)?;
    if let Some(files) = files {
        listed(&c, files)?;
    }
    local::local_user(&mut c, Clock::system().now())?;
    // Even a backup of this very device's: it is not the history the data has gone on with since.
    c.execute("UPDATE epoch SET value = lower(hex(randomblob(16)))", [])?;
    // Of a backup taken as the device synced through a drive: it is in no drive's history now.
    c.execute_batch("DELETE FROM sync_state; DELETE FROM sync_runs;")?;
    // One file, without a -wal beside it.
    c.pragma_update_and_check(None, "journal_mode", "DELETE", |_| Ok(()))?;
    Ok(())
}

/// Checks that `files` are those the database `c` lists, each of the size recorded, with none in use
/// missing; those not in use may be, deleted by the daily clearing as the backup was written.
fn listed(c: &Connection, mut files: HashMap<AttachmentId, u64>) -> anyhow::Result<()> {
    let listed: Vec<(AttachmentId, u32, bool)> = c
        .prepare("SELECT id, size, unused_at IS NULL FROM files")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, size, used) in listed {
        match files.remove(&id) {
            Some(got) if got == u64::from(size) => {}
            None if !used => {}
            _ => bail!(NOT_OURS),
        }
    }
    if !files.is_empty() {
        bail!(NOT_OURS);
    }
    Ok(())
}

/// The user's go-ahead: from here on the backup replaces the data, at the next start. Fails only
/// when it has not committed.
pub fn commit(dir: &Path) -> anyhow::Result<()> {
    std::fs::rename(dir.join(PART), dir.join(READY))?;
    // Committed all the same: losing power before the rename reaches the disk forgets the restore.
    if let Err(e) = sync_dir(dir) {
        tracing::warn!("restore committed, not yet on disk: {e}");
    }
    Ok(())
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
        std::fs::rename(&data, kept(dir, Clock::system().now())?)?;
        sync_dir(dir)?;
    }
    std::fs::rename(ready, data)?;
    Ok(sync_dir(dir)?)
}

/// Where in `dir` to keep the data a restore at `now_ms` replaces: `before-restore-<Unix seconds>`,
/// with `-2`, `-3`… after it when that is taken, by a restore run again within the second say.
pub(crate) fn kept(dir: &Path, now_ms: i64) -> std::io::Result<PathBuf> {
    let name = format!("before-restore-{}", now_ms / 1000);
    let mut kept = dir.join(&name);
    for n in 2.. {
        if !kept.try_exists()? {
            break;
        }
        kept = dir.join(format!("{name}-{n}"));
    }
    Ok(kept)
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
