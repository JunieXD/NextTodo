mod common;

use axum::http::StatusCode;
use common::{TestApp, id};
use nexttodo_server::{AppState, Clock, Db, auth, db};
use rusqlite::Connection;
use serde_json::json;

#[tokio::test]
async fn snapshots_hold_the_data_and_only_the_newest_are_kept() {
    let dir = std::env::temp_dir().join(format!("nexttodo-snapshot-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nexttodo.db");
    let state = AppState::new(Db::open(&file).unwrap(), Clock::system());
    auth::create_admin(&state, "alice".into(), "Alice".into(), "correct horse".into()).await.unwrap();
    let snapshots = dir.join("snapshots");
    // A partial copy left by a crash doesn't get in the way; files put there by hand stay.
    std::fs::create_dir_all(&snapshots).unwrap();
    std::fs::write(snapshots.join("snapshot-0000000009.db.partial"), "half").unwrap();
    let mine = ["before-upgrade.db", "notes.partial"];
    for name in mine {
        std::fs::write(snapshots.join(name), "mine").unwrap();
    }

    let mut last = None;
    for day in 1..=9 {
        last = Some(db::snapshot(&file, &snapshots, day * 86_400_000, 7).unwrap());
    }

    let mut names: Vec<String> = std::fs::read_dir(&snapshots).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    let mut expected: Vec<String> = (3..=9).map(|day| format!("snapshot-{:010}.db", day * 86_400)).collect();
    expected.extend(mine.map(String::from));
    expected.sort();
    assert_eq!(names, expected);
    let copy = Connection::open(last.unwrap()).unwrap();
    let user: String = copy.query_row("SELECT username FROM users", [], |r| r.get(0)).unwrap();
    assert_eq!(user, "alice");
    // Closed first: Windows deletes no file that is open.
    drop((copy, state));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn restoring_a_snapshot_has_devices_download_everything_again() {
    let dir = std::env::temp_dir().join(format!("nexttodo-restore-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nexttodo.db");
    let t = TestApp::on(Db::open(&file).unwrap());
    let s = t.admin().await;
    add(&t, &s, "快照里的").await;
    let snapshot = db::snapshot(&file, &dir.join("snapshots"), 1_000, 7).unwrap();
    add(&t, &s, "快照之后的").await;
    let seen = t.snapshot(&s).await["seq"].as_i64().unwrap();
    // The server stopped.
    drop(t);

    let kept = db::restore(&file, &snapshot, 2_000).unwrap().unwrap();
    let t = TestApp::on(Db::open(&file).unwrap());
    let stale = t.call("GET", &format!("/changes?since={seen}"), Some(&s), None).await;
    assert_eq!((stale.status, stale.body["code"].as_str()), (StatusCode::GONE, Some("resync")));
    let fresh = t.snapshot(&s).await;
    let titles: Vec<&str> = fresh["tasks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["快照里的"]);
    let since = fresh["seq"].as_i64().unwrap();
    assert!(since > seen);
    // Changes go on after it, and reach a device that downloaded it.
    let changed = add(&t, &s, "恢复之后的").await;
    assert!(changed > since);
    let after = t.call("GET", &format!("/changes?since={since}"), Some(&s), None).await.body;
    assert_eq!(after["changes"].as_array().unwrap().len(), 1);
    let replaced = Connection::open(kept.join("nexttodo.db")).unwrap();
    assert_eq!(replaced.query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    drop((t, replaced));

    // With the database replaced damaged, then gone, all the same; the damaged one is kept as it was.
    std::fs::write(&file, "damaged").unwrap();
    let kept = db::restore(&file, &snapshot, 3_000).unwrap().unwrap();
    assert_eq!(std::fs::read(kept.join("nexttodo.db")).unwrap(), b"damaged");
    std::fs::remove_file(&file).unwrap();
    assert_eq!(db::restore(&file, &snapshot, 4_000).unwrap(), None);
    let t = TestApp::on(Db::open(&file).unwrap());
    let stale = t.call("GET", &format!("/changes?since={changed}"), Some(&s), None).await;
    assert_eq!((stale.status, stale.body["code"].as_str()), (StatusCode::GONE, Some("resync")));

    // Not from a file that isn't NextTodo's database, nor from one missing; nothing changes then.
    let fake = Connection::open(dir.join("fake.db")).unwrap();
    fake.execute_batch(
        "CREATE TABLE users (id TEXT, pruned_seq INTEGER);
         CREATE TABLE mutations (seq INTEGER PRIMARY KEY AUTOINCREMENT, ops TEXT, effects TEXT, reply TEXT);
         CREATE TABLE epoch (value TEXT);
         PRAGMA user_version = 4;",
    )
    .unwrap();
    drop(fake);
    std::fs::write(dir.join("empty.db"), "").unwrap();
    for from in ["fake.db", "empty.db", "missing.db"] {
        assert!(db::restore(&file, &dir.join(from), 5_000).is_err(), "{from}");
    }
    assert!(!dir.join("missing.db").exists());
    assert_eq!(t.snapshot(&s).await["tasks"].as_array().unwrap().len(), 1);
    drop(t);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Adds a task; returns the change's sequence number.
async fn add(t: &TestApp, secret: &str, title: &str) -> i64 {
    t.mutate(secret, json!([{ "op": "task.create", "id": id(), "title": title }])).await.body["seq"].as_i64().unwrap()
}
