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
    clean([dir]);
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
    clean([dir]);
}

#[tokio::test]
async fn a_database_with_more_users_is_not_opened() {
    let dir = data_dir();
    drop(existing(&dir, &["alice", "bob"]).await);
    let e = Local::start(&dir).await.err().unwrap();
    assert_eq!(e.to_string(), "数据库里有多个用户，不能在本机使用");
    assert_eq!(count(&dir, "users"), 2);
    clean([dir]);
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
        clean([dir]);
    }
    let dir = data_dir();
    std::fs::create_dir_all(dir.join("db")).unwrap();
    std::fs::write(db_file(&dir), "not a database, but long enough to have a header the size SQLite reads first.....").unwrap();
    assert_eq!(Local::start(&dir).await.err().unwrap().to_string(), "数据库损坏");
    clean([dir]);
}

#[tokio::test]
async fn a_new_session_revokes_the_earlier_ones() {
    let dir = data_dir();
    let local = Local::start(&dir).await.unwrap();
    let first = local.session().await.unwrap();
    let second = local.session().await.unwrap();
    assert_eq!(call(&local, "GET", "/me", &first, None).await.0, 401);
    assert_eq!(call(&local, "GET", "/me", &second, None).await.0, 200);
    clean([dir]);
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
    clean([dir]);
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
    clean([dir, other]);
}

async fn add(local: &Local, session: &str, title: &str) {
    assert_eq!(call(local, "POST", "/tools/add", session, Some(json!({ "title": title }))).await.0, 200);
}

/// The titles of the tasks, in the order of the `next` tool.
async fn titles(local: &Local) -> Vec<String> {
    let session = local.session().await.unwrap();
    let next = call(local, "POST", "/tools/next", &session, Some(json!({}))).await.1;
    next["data"]["tasks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap().to_owned()).collect()
}

/// Deletes what a test leaves. On Windows the files a server still running in the test's process has
/// open can't be, which is no failure of the test.
fn clean(paths: impl IntoIterator<Item = PathBuf>) {
    for path in paths {
        let _ = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

#[tokio::test]
async fn a_backup_replaces_the_data_with_a_new_epoch_and_the_data_replaced_is_kept() {
    let (mine, other, file) = (data_dir(), data_dir(), data_dir().with_extension("db"));
    let local = Local::start(&mine).await.unwrap();
    let session = local.session().await.unwrap();
    add(&local, &session, "备份前").await;
    // Over a file that is there already.
    std::fs::write(&file, "an older backup").unwrap();
    local.backup(file.clone()).await.unwrap();
    // Not over the database, nor anywhere else in the data directory.
    for into in [db_file(&mine), mine.join("backup.db")] {
        assert_eq!(local.backup(into).await.unwrap_err().to_string(), "备份不能放在 NextTodo 的数据目录里");
    }
    add(&local, &session, "备份后").await;

    // The data replaced, with nothing running on it, as when the app has restarted.
    drop(existing(&other, &["alice"]).await);
    nexttodo_server::backup::prepare(&other, &file).unwrap();
    nexttodo_server::backup::commit(&other).unwrap();
    let restored = Local::start(&other).await.unwrap();
    assert_eq!(titles(&restored).await, ["备份前"]);
    assert_eq!(restored.user, local.user);
    assert_ne!(restored.epoch, local.epoch);
    let kept = names(&other).into_iter().find(|n| n.starts_with("before-restore-")).unwrap();
    let c = Connection::open(other.join(kept).join("nexttodo.db")).unwrap();
    assert_eq!(c.query_row("SELECT username FROM users", [], |r| r.get::<_, String>(0)).unwrap(), "alice");
    drop(c);
    clean([mine, other, file]);
}

#[tokio::test]
async fn a_restore_goes_on_where_it_was_cut_short() {
    let (source, uncommitted, cut, file) = (data_dir(), data_dir(), data_dir(), data_dir().with_extension("db"));
    let local = Local::start(&source).await.unwrap();
    let session = local.session().await.unwrap();
    add(&local, &session, "备份里的").await;
    local.backup(file.clone()).await.unwrap();

    // Prepared but never committed: it goes, and the data stays.
    drop(existing(&uncommitted, &["alice"]).await);
    nexttodo_server::backup::prepare(&uncommitted, &file).unwrap();
    let started = Local::start(&uncommitted).await.unwrap();
    assert_ne!(started.user, local.user);
    assert!(!names(&uncommitted).contains(&"restore.part".to_owned()));

    // Cut short once the data replaced was moved away.
    drop(existing(&cut, &["alice"]).await);
    nexttodo_server::backup::prepare(&cut, &file).unwrap();
    nexttodo_server::backup::commit(&cut).unwrap();
    std::fs::rename(cut.join("db"), cut.join("before-restore-1")).unwrap();
    let restored = Local::start(&cut).await.unwrap();
    assert_eq!(titles(&restored).await, ["备份里的"]);
    clean([source, uncommitted, cut, file]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_restore_that_cannot_go_on_deletes_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, file) = (data_dir(), data_dir().with_extension("db"));
    let local = Local::start(&dir).await.unwrap();
    local.backup(file.clone()).await.unwrap();
    drop(local);
    nexttodo_server::backup::prepare(&dir, &file).unwrap();
    nexttodo_server::backup::commit(&dir).unwrap();
    let before = names(&dir);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let e = Local::start(&dir).await.err().unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(e.to_string().starts_with("无法完成恢复"), "{e}");
    assert_eq!(names(&dir), before);
    assert!(Local::start(&dir).await.is_ok());
    clean([dir, file]);
}

#[tokio::test]
async fn a_backup_that_wont_do_changes_nothing() {
    let dir = data_dir();
    let local = Local::start(&dir).await.unwrap();
    let epoch = local.epoch.clone();
    drop(local);
    let source = data_dir();
    let newer = source.join("newer.db");
    let other = source.join("other.db");
    let two = data_dir();
    drop(existing(&two, &["alice", "bob"]).await);
    let none = data_dir();
    drop(existing(&none, &[]).await);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("text.db"), "not a database, but long enough to have a header the size SQLite reads first.....").unwrap();
    std::fs::write(source.join("empty.db"), "").unwrap();
    let c = Connection::open(&other).unwrap();
    c.execute_batch("CREATE TABLE users (id TEXT); INSERT INTO users VALUES ('x'); PRAGMA user_version = 4").unwrap();
    std::fs::copy(db_file(&two), &newer).unwrap();
    Connection::open(&newer).unwrap().pragma_update(None, "user_version", 99).unwrap();
    let not_ours = "这不是 NextTodo 的备份，或者文件已损坏";
    for (file, error) in [
        (source.join("text.db"), not_ours),
        (source.join("empty.db"), not_ours),
        (other, not_ours),
        (newer, "数据来自更高版本的 NextTodo，请先更新"),
        (db_file(&two), "备份里有多个用户，不能在本机使用"),
        (db_file(&none), "备份里没有数据"),
    ] {
        let e = nexttodo_server::backup::prepare(&dir, &file).unwrap_err();
        assert_eq!(e.to_string(), error, "{}", file.display());
        assert!(!names(&dir).contains(&"restore.part".to_owned()));
    }
    assert_eq!(Local::start(&dir).await.unwrap().epoch, epoch);
    clean([dir, source, two, none]);
}

#[tokio::test]
async fn earlier_versions_and_a_servers_database_restore() {
    // A server's, with one user who has a password and is its administrator.
    let server = data_dir();
    drop(existing(&server, &["alice"]).await);
    // Made before the epoch came in.
    let older = data_dir().with_extension("db");
    let c = Connection::open(&older).unwrap();
    for sql in [
        include_str!("../migrations/001_init.sql"),
        include_str!("../migrations/002_ai.sql"),
        include_str!("../migrations/003_custom_ecnu.sql"),
    ] {
        c.execute_batch(sql).unwrap();
    }
    c.execute_batch(
        "PRAGMA user_version = 3;
         INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES ('u', 'alice', 'Alice', 'x', 0);
         INSERT INTO tasks (id, owner_id, number, title, sort_key, created_at, updated_at) VALUES ('01900000-0000-7000-8000-000000000001', 'u', 1, '旧版本的', 'a0', 0, 0);",
    )
    .unwrap();
    drop(c);
    let dirs = [data_dir(), data_dir()];
    for (dir, (file, title)) in dirs.iter().zip([(db_file(&server), None), (older.clone(), Some("旧版本的"))]) {
        nexttodo_server::backup::prepare(dir, &file).unwrap();
        nexttodo_server::backup::commit(dir).unwrap();
        let local = Local::start(dir).await.unwrap();
        let session = local.session().await.unwrap();
        assert_eq!(call(&local, "GET", "/me", &session, None).await.1["username"], "local");
        assert_eq!(titles(&local).await, title.into_iter().collect::<Vec<_>>());
    }
    clean(dirs.into_iter().chain([server, older]));
}
