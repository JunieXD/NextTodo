//! Snapshots of the shared data (docs/local.md#网盘里的文件): the database without what is this
//! device's alone, gzipped. A drive's first generation starts from one, and a device joining reads
//! the latest.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use rusqlite::Connection;
use uuid::Uuid;

use crate::{Db, db};

/// The shared data in `db` now, gzipped. The copy is made in `dir` in one step, holding the
/// database only for that, and deleted after.
pub(crate) fn take(db: &Db, dir: &Path) -> anyhow::Result<Vec<u8>> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("snapshot-{}.db", Uuid::now_v7()));
    let taken = db.hold(|c| db::copy(c, &path)).and_then(|()| {
        clear(&Connection::open(&path)?)?;
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        std::io::copy(&mut File::open(&path)?, &mut gz)?;
        Ok(gz.finish()?)
    });
    // What failed is what to tell.
    let _ = std::fs::remove_file(&path);
    taken
}

/// Clears what is a device's own (docs/local.md#共享的和本机的) from a copy of the database, and
/// compacts it, so that nothing of that is left in the file. The tables of one row stay: whoever reads
/// the snapshot gives it an epoch of its own, and the settings are what they were.
fn clear(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "DELETE FROM tokens;
         DELETE FROM invites;
         DELETE FROM ai_settings;
         DELETE FROM deliveries;
         DELETE FROM sync_state;
         DELETE FROM sync_runs;
         DELETE FROM files WHERE staged;
         UPDATE mutations SET token_id = '', reply = NULL;
         UPDATE users SET pruned_seq = 0;
         VACUUM;",
    )
}

/// The tables of the shared entities, each after those it refers to (docs/local.md#共享的和本机的).
/// `files`, which `attachments` refer to, keeps what is this device's own, and the log its numbering.
const ENTITIES: &[&str] = &["folders", "tasks", "dependencies", "members", "assignees", "ai_jobs", "attachments", "ai_notes"];

/// Replaces the shared data in `c` with that of the snapshot database at `path`, read up to commit
/// `number`, in one transaction; what is this device's own stays (docs/local.md#运行).
pub(crate) fn swap(c: &mut Connection, path: &Path, number: i64) -> rusqlite::Result<()> {
    c.execute("ATTACH DATABASE ?1 AS s", [path.to_str().expect("data directory path is UTF-8")])?;
    let swapped = replace(c, number);
    c.execute("DETACH DATABASE s", [])?;
    swapped
}

fn replace(c: &mut Connection, number: i64) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    for table in ENTITIES.iter().rev() {
        tx.execute(&format!("DELETE FROM main.{table}"), [])?;
    }
    tx.execute_batch(
        "DELETE FROM main.files WHERE NOT staged OR id IN (SELECT id FROM s.files);
         INSERT INTO main.files SELECT * FROM s.files;",
    )?;
    for table in ENTITIES {
        tx.execute(&format!("INSERT INTO main.{table} SELECT * FROM s.{table}"), [])?;
    }
    tx.execute_batch(
        "UPDATE main.users SET next_task_number = (SELECT next_task_number FROM s.users);
         DELETE FROM main.mutations WHERE id NOT IN (SELECT id FROM s.mutations);
         -- Those here keep their numbers, which the replies of tools name; the others are numbered on.
         INSERT INTO main.mutations (id, owner_id, token_id, actor, ops, effects, reply, created_at)
           SELECT id, owner_id, token_id, actor, ops, effects, reply, created_at FROM s.mutations
           WHERE id NOT IN (SELECT id FROM main.mutations) ORDER BY seq;
         DELETE FROM main.deliveries;
         INSERT INTO main.deliveries (user_id, seq, effects) SELECT owner_id, seq, NULL FROM main.mutations WHERE effects IS NOT NULL;
         -- The pages here download everything again.
         UPDATE main.users SET pruned_seq = (SELECT coalesce(max(seq), 0) FROM main.mutations);",
    )?;
    tx.execute("UPDATE sync_state SET applied = ?1", [number])?;
    tx.commit()
}

/// Writes the database in `snapshot`, gzipped, to `path`.
pub(crate) fn unpack(snapshot: &[u8], path: &Path) -> anyhow::Result<()> {
    let mut bytes = Vec::new();
    GzDecoder::new(snapshot).read_to_end(&mut bytes)?;
    let mut file = File::create(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}
