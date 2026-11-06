//! Local mode: the server the app starts for itself on a data directory, reached over HTTP.

use std::path::{Path, PathBuf};

use nexttodo_server::local::Local;
use nexttodo_server::{AppState, Clock, Db, auth};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn data_dir() -> PathBuf {
    std::env::temp_dir().join(format!("nexttodo-local-{}", uuid::Uuid::now_v7()))
}

fn db_file(dir: &Path) -> PathBuf {
    dir.join("db").join("nexttodo.db")
}

/// A database left by a server, or by an earlier version of the app, with the given users.
async fn existing(dir: &Path, users: &[&str]) -> AppState {
    std::fs::create_dir_all(dir.join("db")).unwrap();
    let state = AppState::new(Db::open(&db_file(dir)).unwrap(), Clock::system());
    for name in users {
        auth::create_admin(&state, (*name).into(), (*name).into(), "correct horse".into()).await.unwrap();
    }
    state
}

async fn call(local: &Local, method: &str, path: &str, token: &str, body: Option<Value>) -> (u16, Value) {
    let (url, token) = (format!("{}/api/v1{path}", local.url), token.to_owned());
    let method = method.to_owned();
    tokio::task::spawn_blocking(move || {
        // Straight to it, whatever proxy the environment sets.
        let agent: ureq::Agent = ureq::Agent::config_builder().proxy(None).http_status_as_error(false).build().into();
        let req = ureq::http::Request::builder().method(method.as_str()).uri(url).header("Authorization", format!("Bearer {token}"));
        let mut res = match body {
            Some(body) => agent.run(req.header("Content-Type", "application/json").body(body.to_string()).unwrap()),
            None => agent.run(req.body(()).unwrap()),
        }
        .unwrap();
        let text = res.body_mut().read_to_string().unwrap();
        (res.status().as_u16(), if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap() })
    })
    .await
    .unwrap()
}

fn count(dir: &Path, table: &str) -> i64 {
    Connection::open(db_file(dir)).unwrap().query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

#[tokio::test]
async fn a_new_device_gets_a_local_user_who_never_signs_in() {
    let dir = data_dir();
    let local = Local::start(&dir).await.unwrap();
    let session = local.session().await.unwrap();
    let (status, me) = call(&local, "GET", "/me", &session, None).await;
    assert_eq!(status, 200);
    assert_eq!(me, json!({ "id": local.user, "username": "local", "display_name": "本机", "is_admin": false }));
    for password in ["", "correct horse"] {
        let login = json!({ "username": "local", "password": password, "device": "浏览器" });
        assert_eq!(call(&local, "POST", "/auth/login", "", Some(login)).await.0, 401);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_database_from_a_server_becomes_the_local_users() {
    let dir = data_dir();
    let state = existing(&dir, &["alice"]).await;
    let c = Connection::open(db_file(&dir)).unwrap();
    let alice: String = c.query_row("SELECT id FROM users", [], |r| r.get(0)).unwrap();
    c.execute("INSERT INTO invites (code, created_by, created_at) VALUES ('abc', ?1, 0)", [&alice]).unwrap();
    c.execute(
        "INSERT INTO ai_settings (user_id, provider, base_url, model, thinking, api_key, updated_at)
         VALUES (?1, 'openai', 'https://api.openai.com/v1', 'gpt', 'off', x'00', 0)",
        [&alice],
    )
    .unwrap();
    c.execute(
        "INSERT INTO tokens (id, user_id, kind, name, scope, secret_hash, created_at) VALUES ('t', ?1, 'api', 'Claude Code', 'write', ?2, 0)",
        params![alice, Sha256::digest("nt_agent").to_vec()],
    )
    .unwrap();
    drop((c, state));

    let local = Local::start(&dir).await.unwrap();
    assert_eq!(local.user, alice);
    let session = local.session().await.unwrap();
    assert_eq!(call(&local, "GET", "/me", &session, None).await.1["username"], "local");
    assert_eq!(call(&local, "GET", "/me", "nt_agent", None).await.0, 200);
    let login = json!({ "username": "local", "password": "correct horse", "device": "浏览器" });
    assert_eq!(call(&local, "POST", "/auth/login", "", Some(login)).await.0, 401);
    assert_eq!(call(&local, "GET", "/me", &session, None).await.1["is_admin"], false);
    assert_eq!((count(&dir, "invites"), count(&dir, "ai_settings")), (0, 0));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_database_with_more_users_is_not_opened() {
    let dir = data_dir();
    drop(existing(&dir, &["alice", "bob"]).await);
    let e = Local::start(&dir).await.err().unwrap();
    assert_eq!(e.to_string(), "数据库里有多个用户，不能在本机使用");
    assert_eq!(count(&dir, "users"), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_database_not_ours_or_from_a_newer_version_is_left_alone() {
    for (version, error) in [(99, "数据来自更高版本的 NextTodo，请先更新"), (-1, "数据库损坏")] {
        let dir = data_dir();
        std::fs::create_dir_all(dir.join("db")).unwrap();
        let c = Connection::open(db_file(&dir)).unwrap();
        c.execute_batch("CREATE TABLE notes (text TEXT)").unwrap();
        c.pragma_update(None, "user_version", version).unwrap();
        drop(c);
        let before = std::fs::read(db_file(&dir)).unwrap();
        assert_eq!(Local::start(&dir).await.err().unwrap().to_string(), error);
        assert_eq!(std::fs::read(db_file(&dir)).unwrap(), before);
        let mut files: Vec<_> = std::fs::read_dir(dir.join("db")).unwrap().map(|e| e.unwrap().file_name()).collect();
        files.sort();
        assert_eq!(files, ["nexttodo.db"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
    let dir = data_dir();
    std::fs::create_dir_all(dir.join("db")).unwrap();
    std::fs::write(db_file(&dir), "not a database, but long enough to have a header the size SQLite reads first.....").unwrap();
    assert_eq!(Local::start(&dir).await.err().unwrap().to_string(), "数据库损坏");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_new_session_revokes_the_earlier_ones() {
    let dir = data_dir();
    let local = Local::start(&dir).await.unwrap();
    let first = local.session().await.unwrap();
    let second = local.session().await.unwrap();
    assert_eq!(call(&local, "GET", "/me", &first, None).await.0, 401);
    assert_eq!(call(&local, "GET", "/me", &second, None).await.0, 200);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn jobs_cut_short_have_failed_by_the_first_request() {
    let dir = data_dir();
    drop(existing(&dir, &["alice"]).await);
    let c = Connection::open(db_file(&dir)).unwrap();
    c.execute(
        "INSERT INTO ai_jobs (id, owner_id, text, local_time, state, created_at, updated_at)
         SELECT '01900000-0000-7000-8000-000000000000', id, '整理', '2026-11-06 10:00 星期五', 'running', 0, 0 FROM users",
        [],
    )
    .unwrap();
    drop(c);
    let local = Local::start(&dir).await.unwrap();
    let session = local.session().await.unwrap();
    let snapshot = call(&local, "GET", "/snapshot", &session, None).await.1;
    assert_eq!(snapshot["ai_jobs"][0]["state"], "failed");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn the_epoch_stays_with_the_database() {
    let (dir, other) = (data_dir(), data_dir());
    let first = Local::start(&dir).await.unwrap();
    let epoch = first.epoch.clone();
    // Its lock goes with it, as when the process ends.
    drop(first);
    assert_eq!(Local::start(&dir).await.unwrap().epoch, epoch);
    assert_ne!(Local::start(&other).await.unwrap().epoch, epoch);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(other).unwrap();
}
