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

/// What a snapshot keeps of each table (docs/local.md#共享的和本机的). Every table is named, and every
/// column of those partly this device's own: a test fails on one that is not, so that whoever adds it
/// says which it is. The columns of an entity's table are all shared: they go in the drive's commits.
const TABLES: &[(&str, Kept)] = &[
    ("ai_jobs", Kept::Shared),
    ("ai_notes", Kept::Shared),
    ("ai_settings", Kept::Own),
    ("assignees", Kept::Shared),
    ("attachments", Kept::Shared),
    ("deliveries", Kept::Own),
    ("dependencies", Kept::Shared),
    // A new one is given by whoever reads the snapshot.
    ("epoch", Kept::Partly(&[("id", None), ("value", Some("''"))])),
    // Without those uploaded here and not attached in a commit yet (`clear`).
    ("files", Kept::Shared),
    ("folders", Kept::Shared),
    ("invites", Kept::Own),
    ("members", Kept::Shared),
    (
        "mutations",
        Kept::Partly(&[
            ("seq", None),
            ("id", None),
            ("owner_id", None),
            ("token_id", Some("''")),
            ("actor", None),
            ("ops", None),
            ("effects", None),
            ("reply", Some("NULL")),
            ("created_at", None),
        ]),
    ),
    ("server_settings", Kept::Partly(&[("id", None), ("compress_images", Some("1"))])),
    ("sync_runs", Kept::Own),
    ("sync_state", Kept::Own),
    ("tasks", Kept::Shared),
    ("tokens", Kept::Own),
    (
        "users",
        Kept::Partly(&[
            ("id", None),
            ("username", None),
            ("display_name", None),
            ("password_hash", Some("''")),
            ("is_admin", None),
            ("next_task_number", None),
            ("pruned_seq", Some("0")),
            ("created_at", None),
        ]),
    ),
];

enum Kept {
    /// Shared: as it is.
    Shared,
    /// Shared but for the columns given a value, which are this device's own and are set to it.
    Partly(&'static [(&'static str, Option<&'static str>)]),
    /// This device's own: left empty.
    Own,
}

/// Clears what is a device's own from a copy of the database, and compacts it, so that nothing of
/// that is left in the file.
fn clear(c: &Connection) -> rusqlite::Result<()> {
    for (table, kept) in TABLES {
        match kept {
            Kept::Shared => {}
            Kept::Own => _ = c.execute(&format!("DELETE FROM {table}"), [])?,
            Kept::Partly(columns) => {
                let set: Vec<String> = columns.iter().filter_map(|(column, value)| Some(format!("{column} = {}", (*value)?))).collect();
                c.execute(&format!("UPDATE {table} SET {}", set.join(", ")), [])?;
            }
        }
    }
    c.execute_batch("DELETE FROM files WHERE staged; VACUUM;")
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Read;

    use flate2::read::GzDecoder;
    use rusqlite::Connection;

    use super::{Kept, TABLES};
    use crate::Db;

    #[test]
    fn every_table_and_every_column_of_those_partly_this_devices_own_is_said_to_be_shared_or_not() {
        let db = Db::open_in_memory().unwrap();
        let schema: BTreeMap<String, Vec<String>> = db.hold(|c| {
            let tables: Vec<String> = c
                .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            tables.into_iter().map(|t| (t.clone(), columns(c, &t))).collect()
        });
        let said: BTreeMap<String, Vec<String>> = TABLES
            .iter()
            .map(|(table, kept)| {
                let named = match kept {
                    Kept::Partly(columns) => columns.iter().map(|(c, _)| (*c).to_owned()).collect(),
                    Kept::Shared | Kept::Own => schema.get(*table).cloned().unwrap_or_default(),
                };
                ((*table).to_owned(), named)
            })
            .collect();
        assert_eq!(said, schema);
    }

    fn columns(c: &Connection, table: &str) -> Vec<String> {
        c.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn a_snapshot_has_nothing_of_what_is_this_devices_own() {
        let (db, dir) = (Db::open_in_memory().unwrap(), std::env::temp_dir().join(format!("nexttodo-snapshot-{}", uuid::Uuid::now_v7())));
        db.hold(|c| {
            let user = crate::local::local_user(c, 1).unwrap();
            c.execute_batch(&format!(
                "UPDATE users SET password_hash = 'PASSWORD-SECRET';
                 INSERT INTO tokens (id, user_id, kind, name, scope, secret_hash, created_at) VALUES ('t', '{user}', 'api', 'agent', 'write', CAST('TOKEN-SECRET' AS BLOB), 1);
                 INSERT INTO ai_settings (user_id, provider, base_url, model, thinking, api_key, updated_at) VALUES ('{user}', 'deepseek', 'https://api.deepseek.com', 'm', 'off', CAST('sk-SECRET' AS BLOB), 1);
                 INSERT INTO tasks (id, owner_id, number, title, sort_key, created_at, updated_at) VALUES ('019a4b2c-0000-7000-8000-000000000001', '{user}', 1, 'SHARED-TITLE', 'a', 1, 1);
                 INSERT INTO mutations (id, owner_id, token_id, actor, ops, effects, reply, created_at) VALUES ('m', '{user}', 't', 'agent', '[]', '[]', 'REPLY-SECRET', 1);"
            ))
            .unwrap();
        });
        let mut bytes = Vec::new();
        GzDecoder::new(super::take(&db, &dir).unwrap().as_slice()).read_to_end(&mut bytes).unwrap();
        let has = |s: &str| bytes.windows(s.len()).any(|w| w == s.as_bytes());
        assert!(has("SHARED-TITLE"));
        for secret in ["PASSWORD-SECRET", "TOKEN-SECRET", "sk-SECRET", "REPLY-SECRET"] {
            assert!(!has(secret), "{secret}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
