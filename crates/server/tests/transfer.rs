//! Moving data between local mode and a server account: an account's export, and a backup imported
//! into one (docs/local.md#搬到服务器和搬回本机).

mod common;

use std::io::Read;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{Reply, TestApp, id};
use rusqlite::Connection;
use serde_json::{Map, Value, json};

/// Alice's data on a server, and what others have there: `/工作` with `/工作/周报` in it, `/共享`
/// shared with Bob, who added a task to it, and Bob's `/Bob 的`, shared with Alice, where she added
/// a task and a job of her own.
struct Source {
    t: TestApp,
    alice: String,
    attachment: String,
}

async fn source() -> Source {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    let (work, weekly, shared, theirs) = (id(), id(), id(), id());
    let (outline, draft, daily, attached) = (id(), id(), id(), id());
    let due = json!({ "date": "2026-11-10", "repeat": { "every": 1, "unit": "day", "from": "due" } });
    let r = t
        .mutate(
            &alice,
            json!([
                { "op": "folder.create", "id": work, "name": "工作" },
                { "op": "folder.create", "id": weekly, "name": "周报", "parent_id": work },
                { "op": "folder.create", "id": shared, "name": "共享" },
                { "op": "task.create", "id": id(), "title": "根目录的事" },
                { "op": "task.create", "id": outline, "title": "写提纲", "folder_id": work },
                { "op": "task.create", "id": draft, "title": "写正文", "folder_id": work },
                { "op": "dep.add", "before": outline, "after": draft },
                { "op": "task.create", "id": daily, "title": "背单词", "folder_id": weekly, "due": due },
                { "op": "task.create", "id": attached, "title": "带附件的", "folder_id": work },
                { "op": "ai_job.create", "id": id(), "text": "下周开会", "local_time": "2026-11-04 16:30 星期三", "folder_id": work },
            ]),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = t.mutate(&alice, json!([{ "op": "task.update", "id": daily, "done": true, "today": "2026-11-10" }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let attachment = t.attach(&alice, &attached, "清单.txt", b"milk").await;
    let description = format!("见 [清单](attachment:{attachment})");
    let r = t.mutate(&alice, json!([{ "op": "task.update", "id": attached, "description": description }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    t.ok(&alice, "share", json!({ "folder": "/共享", "users": ["bob"] })).await;
    let r = t.mutate(&bob, json!([{ "op": "task.create", "id": id(), "title": "Bob 加的", "folder_id": shared }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = t.mutate(&bob, json!([{ "op": "folder.create", "id": theirs, "name": "Bob 的" }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    t.ok(&bob, "share", json!({ "folder": "/Bob 的", "users": ["alice"] })).await;
    let r = t
        .mutate(
            &alice,
            json!([
                { "op": "task.create", "id": id(), "title": "Alice 在 Bob 那里加的", "folder_id": theirs },
                { "op": "ai_job.create", "id": id(), "text": "Bob 那里的", "local_time": "2026-11-04 16:30 星期三", "folder_id": theirs },
            ]),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for (folder, text) in [(Value::from(work), "工作说明"), (Value::Null, "全局说明"), (Value::from(theirs), "Bob 那里的说明")]
    {
        let r = t.call("PUT", "/ai/notes", Some(&alice), Some(json!({ "folder_id": folder, "text": text }))).await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.body);
    }
    Source { t, alice, attachment }
}

async fn export(t: &TestApp, secret: &str) -> Vec<u8> {
    let req = Request::builder().uri("/api/v1/export?today=2026-11-25").header(header::AUTHORIZATION, format!("Bearer {secret}"));
    let (status, headers, bytes) = t.raw(req, Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(headers[header::CONTENT_DISPOSITION], "attachment; filename*=UTF-8''NextTodo-alice-2026-11-25.tar");
    bytes
}

/// The entries of a tar, by name, in order.
fn entries(tar: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = tar::Archive::new(tar);
    let read = |e: std::io::Result<tar::Entry<'_, &[u8]>>| {
        let mut e = e.unwrap();
        let mut data = Vec::new();
        e.read_to_end(&mut data).unwrap();
        (e.path().unwrap().display().to_string(), data)
    };
    archive.entries().unwrap().map(read).collect()
}

fn temp(extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nexttodo-transfer-{}.{extension}", id()))
}

/// A tar's database, written to a file of its own to be read.
fn database(tar: &[u8]) -> PathBuf {
    let path = temp("db");
    std::fs::write(&path, &entries(tar).into_iter().find(|(name, _)| name == "nexttodo.db").unwrap().1).unwrap();
    path
}

fn column(c: &Connection, sql: &str) -> Vec<String> {
    c.prepare(sql).unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}

#[tokio::test]
async fn an_export_is_the_accounts_own_data_and_restores_in_local_mode() {
    let s = source().await;
    let tar = export(&s.t, &s.alice).await;
    let names: Vec<String> = entries(&tar).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["backup.json".to_owned(), format!("files/{}", s.attachment), "nexttodo.db".to_owned()]);

    let path = database(&tar);
    let c = Connection::open(&path).unwrap();
    let alice = s.t.me(&s.alice).await;
    assert_eq!(column(&c, "SELECT id || ' ' || username || ' ' || password_hash FROM users"), [format!("{alice} alice ")]);
    assert_eq!(column(&c, "SELECT name FROM folders ORDER BY name"), ["共享", "周报", "工作"]);
    let titles = column(&c, "SELECT title FROM tasks ORDER BY number");
    assert_eq!(titles, ["根目录的事", "写提纲", "写正文", "背单词", "带附件的", "Bob 加的"]);
    assert_eq!(column(&c, "SELECT DISTINCT created_by || updated_by FROM tasks"), [format!("{alice}{alice}")], "the one user there");
    assert_eq!(column(&c, "SELECT text FROM ai_jobs"), ["下周开会"]);
    assert_eq!(column(&c, "SELECT text FROM ai_notes ORDER BY text"), ["全局说明", "工作说明"]);
    for table in ["members", "assignees", "tokens", "invites", "mutations", "ai_settings"] {
        assert_eq!(column(&c, &format!("SELECT count(*) || '' FROM {table}")), ["0"], "{table}");
    }
    drop(c);
    // Not even left over in the file's free space.
    let hash: String =
        s.t.state
            .db
            .call(|c| Ok(c.query_row("SELECT password_hash FROM users WHERE username = 'alice'", [], |r| r.get(0))?))
            .await
            .unwrap();
    let raw = std::fs::read(&path).unwrap();
    let leaked: Vec<&[u8; 16]> =
        hash.as_bytes().as_chunks::<16>().0.iter().filter(|piece| raw.windows(16).any(|w| w == piece.as_slice())).collect();
    assert!(hash.len() > 32 && leaked.is_empty(), "{:?}", leaked.iter().map(|p| String::from_utf8_lossy(p.as_slice())).collect::<Vec<_>>());

    let dir = temp("d");
    nexttodo_server::backup::prepare(&dir, &{
        let file = temp("tar");
        std::fs::write(&file, &tar).unwrap();
        file
    })
    .unwrap();
    nexttodo_server::backup::commit(&dir).unwrap();
    let local = nexttodo_server::local::Local::start(&dir).await.unwrap();
    let restored = Connection::open(dir.join("db").join("nexttodo.db")).unwrap();
    assert_eq!(column(&restored, "SELECT title FROM tasks ORDER BY number"), titles);
    assert_eq!(std::fs::read(dir.join("db").join("files").join(&s.attachment)).unwrap(), b"milk");
    drop((restored, local));
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(path);
}

/// Uploads a backup's files as `secret`'s, each under a new id: what `POST /import` is to be given,
/// but for its id.
async fn upload(t: &TestApp, secret: &str, tar: &[u8]) -> Value {
    let (mut database, mut files) = (Value::Null, Map::new());
    for (name, bytes) in entries(tar) {
        let new = id();
        let media_type = match name.strip_prefix("files/") {
            Some(old) => {
                files.insert(old.to_owned(), new.clone().into());
                "application/octet-stream"
            }
            None if name == "nexttodo.db" => {
                database = new.clone().into();
                "application/vnd.sqlite3"
            }
            None => continue,
        };
        let r = t.upload(secret, &new, media_type, &bytes).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    }
    json!({ "database": database, "files": files })
}

async fn import(t: &TestApp, secret: &str, change: &str, uploaded: &Value) -> Reply {
    let mut body = uploaded.clone();
    body["id"] = change.into();
    t.call("POST", "/import", Some(secret), Some(body)).await
}

fn tasks(snap: &Value) -> Vec<(i64, &str)> {
    let mut tasks: Vec<(i64, &str)> =
        snap["tasks"].as_array().unwrap().iter().map(|t| (t["number"].as_i64().unwrap(), t["title"].as_str().unwrap())).collect();
    tasks.sort();
    tasks
}

fn roots(snap: &Value) -> Vec<&str> {
    let mut names: Vec<&str> =
        snap["folders"].as_array().unwrap().iter().filter(|f| f["parent_id"].is_null()).map(|f| f["name"].as_str().unwrap()).collect();
    names.sort();
    names
}

fn find<'a>(snap: &'a Value, list: &str, key: &str, value: &str) -> &'a Value {
    snap[list].as_array().unwrap().iter().find(|v| v[key] == value).unwrap_or_else(|| panic!("no {list} with {key} {value}"))
}

#[tokio::test]
async fn an_import_adds_the_backup_anew_and_is_undone_whole() {
    let s = source().await;
    let tar = export(&s.t, &s.alice).await;
    let before = s.t.snapshot(&s.alice).await;

    let t = TestApp::new();
    let me = t.admin().await;
    let r = t
        .mutate(
            &me,
            json!([{ "op": "folder.create", "id": id(), "name": "工作" }, { "op": "task.create", "id": id(), "title": "本来就有" }]),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = t.call("PUT", "/ai/notes", Some(&me), Some(json!({ "folder_id": null, "text": "原来的说明" }))).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.body);

    let (first, uploaded) = (id(), upload(&t, &me, &tar).await);
    let r = import(&t, &me, &first, &uploaded).await;
    assert_eq!((r.status, &r.body["tasks"], &r.body["folders"]), (StatusCode::OK, &json!(6), &json!(3)), "{}", r.body);
    let made = r.body;
    let snap = t.snapshot(&me).await;
    assert_eq!(roots(&snap), ["共享", "工作", "工作 副本"], "beside what was there, under a name of its own");
    assert_eq!(
        tasks(&snap),
        [(1, "本来就有"), (2, "根目录的事"), (3, "写提纲"), (4, "写正文"), (5, "背单词"), (6, "带附件的"), (7, "Bob 加的")],
        "numbered after what was there, in the order they were"
    );
    let old: Vec<&Value> =
        ["tasks", "folders", "ai_jobs", "attachments"].iter().flat_map(|l| before[*l].as_array().unwrap()).map(|v| &v["id"]).collect();
    for list in ["tasks", "folders", "ai_jobs", "attachments"] {
        assert!(snap[list].as_array().unwrap().iter().all(|v| !old.contains(&&v["id"])), "{list} have new ids");
    }
    let me_id = t.me(&me).await;
    assert!(snap["tasks"].as_array().unwrap().iter().all(|t| t["created_by"] == me_id.as_str()));
    let (outline, draft) = (find(&snap, "tasks", "title", "写提纲"), find(&snap, "tasks", "title", "写正文"));
    assert_eq!(snap["dependencies"][0]["before"], outline["id"]);
    assert_eq!(snap["dependencies"][0]["after"], draft["id"]);
    let daily = find(&snap, "tasks", "title", "背单词");
    assert_eq!(daily["completions"]["count"], 1);
    assert_eq!(snap["completions"][0]["task_id"], daily["id"]);
    let copy = find(&snap, "folders", "name", "工作 副本");
    assert_eq!(find(&snap, "folders", "name", "周报")["parent_id"], copy["id"]);

    let attached = find(&snap, "tasks", "title", "带附件的");
    let a = &snap["attachments"][0];
    assert_eq!((&a["name"], &a["type"], &a["task_id"]), (&json!("清单.txt"), &json!("text/plain"), &attached["id"]));
    assert_eq!(a["id"], uploaded["files"][&s.attachment]);
    assert_eq!(attached["description"], format!("见 [清单](attachment:{})", a["id"].as_str().unwrap()));
    assert_eq!(t.download(&me, a["id"].as_str().unwrap()).await.2, b"milk");

    let job = &snap["ai_jobs"][0];
    assert_eq!((&job["state"], &job["error"], &job["folder_id"]), (&json!("failed"), &json!("导入的请求，重试后整理"), &copy["id"]));
    let notes = t.call("GET", "/ai/notes", Some(&me), None).await.body;
    let mut notes: Vec<(&Value, &str)> = notes.as_array().unwrap().iter().map(|n| (&n["folder_id"], n["text"].as_str().unwrap())).collect();
    notes.sort_by_key(|n| n.1);
    assert_eq!(notes, [(&Value::Null, "原来的说明"), (&copy["id"], "工作说明")], "a global note already there stays");
    assert_eq!(snap["shares"], json!([]));

    // Sent again, as when the reply was lost: what it did then.
    let r = import(&t, &me, &first, &uploaded).await;
    assert_eq!((r.status, &r.body), (StatusCode::OK, &made));
    assert_eq!(tasks(&t.snapshot(&me).await).len(), 7);
    // Imported again: another copy.
    let r = import(&t, &me, &id(), &upload(&t, &me, &tar).await).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let snap = t.snapshot(&me).await;
    assert_eq!(roots(&snap), ["共享", "共享 副本", "工作", "工作 副本", "工作 副本 2"]);
    assert_eq!(tasks(&snap).last(), Some(&(13, "Bob 加的")));

    t.ok(&me, "undo", json!({ "batch": made["batch"] })).await;
    let snap = t.snapshot(&me).await;
    assert_eq!(roots(&snap), ["共享 副本", "工作", "工作 副本 2"]);
    assert_eq!(tasks(&snap).len(), 7);
    assert_eq!(snap["attachments"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn an_import_that_wont_do_adds_nothing() {
    let s = source().await;
    let tar = export(&s.t, &s.alice).await;
    let t = TestApp::new();
    let me = t.admin().await;
    let uploaded = upload(&t, &me, &tar).await;
    let refused = |r: Reply, message: &str| {
        assert_eq!((r.status, r.body["message"].as_str()), (StatusCode::BAD_REQUEST, Some(message)), "{}", r.body);
    };

    let read = t.call("POST", "/tokens", Some(&me), Some(json!({ "name": "看看", "scope": "read" }))).await.body["secret"].clone();
    assert_eq!(import(&t, read.as_str().unwrap(), &id(), &uploaded).await.status, StatusCode::FORBIDDEN);

    let mut without = uploaded.clone();
    without["files"] = json!({});
    refused(import(&t, &me, &id(), &without).await, "这些附件的文件没有上传成功：清单.txt");
    let mut other = uploaded.clone();
    let wrong = id();
    t.upload(&me, &wrong, "application/octet-stream", b"eggs and milk").await;
    other["files"][&s.attachment] = wrong.into();
    refused(import(&t, &me, &id(), &other).await, "这些附件的文件没有上传成功：清单.txt");
    let mut missing = uploaded.clone();
    missing["database"] = id().into();
    refused(import(&t, &me, &id(), &missing).await, "备份的数据库没有上传");

    let path = database(&tar);
    let edits = [
        (
            "INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES ('x', 'bob', 'Bob', '', 0)",
            "备份里有多个用户，不能导入",
        ),
        ("DELETE FROM users", "备份里没有数据"),
        ("PRAGMA user_version = 100000", "备份来自更新版本的 NextTodo，请先更新服务端"),
        // Folders in one another, shared as the live data's are read; and dependencies round in a circle.
        (
            "INSERT INTO members (folder_id, user_id) SELECT id, (SELECT id FROM users) FROM folders WHERE name = '工作';
             UPDATE folders SET parent_id = (SELECT id FROM folders WHERE name = '周报') WHERE name = '工作'",
            "这不是 NextTodo 的备份，或者文件已损坏",
        ),
        (
            "INSERT INTO dependencies (before_id, after_id, owner_id, created_at) SELECT after_id, before_id, owner_id, created_at FROM dependencies",
            "这不是 NextTodo 的备份，或者文件已损坏",
        ),
    ];
    for (sql, message) in edits {
        let edited = temp("db");
        std::fs::copy(&path, &edited).unwrap();
        Connection::open(&edited).unwrap().execute_batch(sql).unwrap();
        let mut body = uploaded.clone();
        body["database"] = id().into();
        t.upload(&me, body["database"].as_str().unwrap(), "application/vnd.sqlite3", &std::fs::read(&edited).unwrap()).await;
        refused(import(&t, &me, &id(), &body).await, message);
        let _ = std::fs::remove_file(edited);
    }
    let mut body = uploaded.clone();
    body["database"] = id().into();
    t.upload(&me, body["database"].as_str().unwrap(), "application/vnd.sqlite3", b"not a database").await;
    refused(import(&t, &me, &id(), &body).await, "这不是 NextTodo 的备份，或者文件已损坏");

    let snap = t.snapshot(&me).await;
    assert_eq!((snap["tasks"].as_array().unwrap().len(), snap["folders"].as_array().unwrap().len()), (0, 0));
    // What failed took none of the files: the backup imports with them still.
    assert_eq!(import(&t, &me, &id(), &uploaded).await.status, StatusCode::OK);
    let _ = std::fs::remove_file(path);
}
