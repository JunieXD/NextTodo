//! Attachments: uploading files, attaching them, who sees them, what they do when tasks go, move and
//! come back, and clearing away the files no longer used.

mod common;

use std::time::{Duration, SystemTime};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use common::{TestApp, id};
use nexttodo_server::attachments;
use serde_json::{Value, json};

const DAY: i64 = 24 * 3600 * 1000;

async fn task(t: &TestApp, s: &str, title: &str, folder: Option<&str>) -> String {
    let task = id();
    let r = t.mutate(s, json!([{ "op": "task.create", "id": task, "title": title, "folder_id": folder }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    task
}

fn names(snap: &Value) -> Vec<&str> {
    snap["attachments"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect()
}

/// Submits `ops` as mutation `m`, to undo later.
async fn mutate_as(t: &TestApp, s: &str, m: &str, ops: Value) -> common::Reply {
    t.call("POST", "/mutations", Some(s), Some(json!({ "id": m, "ops": ops }))).await
}

async fn undo(t: &TestApp, s: &str, m: &str) -> common::Reply {
    t.mutate(s, json!([{ "op": "undo", "mutation": m }])).await
}

#[tokio::test]
async fn a_file_uploaded_and_attached_downloads_as_a_file_only_its_task_shows() {
    let t = TestApp::new();
    let s = t.admin().await;
    let bob = t.user(&s, "bob").await;
    let task = task(&t, &s, "报价", None).await;
    let a = t.attach(&s, &task, "报价 <终版>.txt", b"hello").await;

    let snap = t.snapshot(&s).await;
    let shown = &snap["attachments"][0];
    assert_eq!((&shown["id"], &shown["task_id"], &shown["job_id"]), (&json!(a), &json!(task), &Value::Null));
    assert_eq!((&shown["name"], &shown["type"], &shown["size"]), (&json!("报价 <终版>.txt"), &json!("text/plain"), &json!(5)));

    let (status, h, bytes) = t.download(&s, &a).await;
    assert_eq!((status, bytes.as_slice()), (StatusCode::OK, &b"hello"[..]));
    assert_eq!(h[header::CONTENT_TYPE], "text/plain");
    assert_eq!(h[header::CONTENT_DISPOSITION], "attachment; filename*=UTF-8''%E6%8A%A5%E4%BB%B7%20%3C%E7%BB%88%E7%89%88%3E.txt");
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(h[header::CONTENT_SECURITY_POLICY], "sandbox");
    assert_eq!(h[header::CACHE_CONTROL], "no-store");

    assert_eq!(t.download(&bob, &a).await.0, StatusCode::NOT_FOUND);
    assert!(names(&t.snapshot(&bob).await).is_empty());
}

#[tokio::test]
async fn uploads_are_checked_and_can_be_sent_again() {
    let t = TestApp::new();
    let s = t.admin().await;
    let bob = t.user(&s, "bob").await;
    let a = id();
    let first = t.upload(&s, &a, "image/png", b"12345").await;
    assert_eq!((first.status, &first.body), (StatusCode::OK, &json!({ "id": a, "size": 5 })));
    // The reply was lost, say: the first upload stands.
    let again = t.upload(&s, &a, "image/png", b"123").await;
    assert_eq!((again.status, &again.body), (StatusCode::OK, &json!({ "id": a, "size": 5 })));
    assert_eq!(t.upload(&bob, &a, "image/png", b"123").await.status, StatusCode::CONFLICT);

    let big = vec![0u8; 20 * 1024 * 1024 + 1];
    let r = t.upload(&s, &id(), "application/octet-stream", &big).await;
    assert_eq!((r.status, r.body["code"].as_str()), (StatusCode::PAYLOAD_TOO_LARGE, Some("too_large")));
    assert_eq!(t.upload(&s, &id(), "image/png", b"").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(t.upload(&s, &id(), "png", b"1").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(t.upload(&s, "not-an-id", "image/png", b"1").await.status, StatusCode::BAD_REQUEST);

    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "看板", "scope": "read" }))).await.body;
    let read_only = token["secret"].as_str().unwrap();
    assert_eq!(t.upload(read_only, &id(), "image/png", b"1").await.status, StatusCode::FORBIDDEN);

    // Only its uploader gets a file not attached yet.
    assert_eq!(t.download(&s, &a).await.0, StatusCode::OK);
    assert_eq!(t.download(&bob, &a).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_ones_own_upload_is_attached_as_it_was_uploaded() {
    let t = TestApp::new();
    let s = t.admin().await;
    let bob = t.user(&s, "bob").await;
    let task = task(&t, &s, "报价", None).await;
    let add = |a: &str, size: usize| json!([{ "op": "attachment.add", "id": a, "name": "a.txt", "type": "text/plain", "size": size, "task_id": task }]);

    let missing = t.mutate(&s, add(&id(), 5)).await;
    assert_eq!((missing.status, missing.body["code"].as_str()), (StatusCode::NOT_FOUND, Some("file_missing")));
    let bobs = id();
    t.upload(&bob, &bobs, "text/plain", b"hello").await;
    assert_eq!(t.mutate(&s, add(&bobs, 5)).await.body["code"], "file_missing");

    let a = id();
    t.upload(&s, &a, "text/plain", b"hello").await;
    assert_eq!(t.mutate(&s, add(&a, 4)).await.status, StatusCode::BAD_REQUEST);
    let both =
        json!([{ "op": "attachment.add", "id": a, "name": "a.txt", "type": "text/plain", "size": 5, "task_id": task, "job_id": id() }]);
    assert_eq!(t.mutate(&s, both).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(t.mutate(&s, add(&a, 5)).await.status, StatusCode::OK);
    assert_eq!(t.mutate(&s, add(&a, 5)).await.status, StatusCode::CONFLICT);
    // Someone else's task: as if it weren't there.
    let other = task_of(&t, &bob).await;
    let c = id();
    t.upload(&s, &c, "text/plain", b"x").await;
    let r =
        t.mutate(&s, json!([{ "op": "attachment.add", "id": c, "name": "a", "type": "text/plain", "size": 1, "task_id": other }])).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

async fn task_of(t: &TestApp, s: &str) -> String {
    task(t, s, "别人的", None).await
}

#[tokio::test]
async fn attachments_go_and_come_back_with_their_task_and_renames_undo() {
    let t = TestApp::new();
    let s = t.admin().await;
    let task = task(&t, &s, "报价", None).await;
    let a = t.attach(&s, &task, "a.txt", b"hello").await;

    let renamed = id();
    mutate_as(&t, &s, &renamed, json!([{ "op": "attachment.update", "id": a, "name": "报价单.txt" }])).await;
    assert_eq!(names(&t.snapshot(&s).await), ["报价单.txt"]);

    let deleted = id();
    let r = mutate_as(&t, &s, &deleted, json!([{ "op": "task.delete", "id": task }])).await;
    let kinds: Vec<&str> = r.body["effects"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["attachment", "task"]);
    assert!(names(&t.snapshot(&s).await).is_empty());

    assert_eq!(undo(&t, &s, &deleted).await.status, StatusCode::OK);
    assert_eq!(names(&t.snapshot(&s).await), ["报价单.txt"]);
    assert_eq!(t.download(&s, &a).await.2, b"hello");
    assert_eq!(undo(&t, &s, &renamed).await.status, StatusCode::OK);
    assert_eq!(names(&t.snapshot(&s).await), ["a.txt"]);

    // A folder takes its tasks' attachments along too.
    let folder = id();
    t.mutate(&s, json!([{ "op": "folder.create", "id": folder, "name": "工作" }])).await;
    t.mutate(&s, json!([{ "op": "task.update", "id": task, "folder_id": folder }])).await;
    let gone = id();
    mutate_as(&t, &s, &gone, json!([{ "op": "folder.delete", "id": folder }])).await;
    assert!(names(&t.snapshot(&s).await).is_empty());
    assert_eq!(undo(&t, &s, &gone).await.status, StatusCode::OK);
    assert_eq!(names(&t.snapshot(&s).await), ["a.txt"]);
}

#[tokio::test]
async fn members_see_attachments_while_the_task_is_in_the_shared_folder() {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    let carol = t.user(&alice, "carol").await;
    let (home, private) = (id(), id());
    t.mutate(
        &alice,
        json!([
            { "op": "folder.create", "id": home, "name": "装修" },
            { "op": "folder.create", "id": private, "name": "私人" },
        ]),
    )
    .await;
    t.ok(&alice, "share", json!({ "folder": "/装修", "users": ["bob"] })).await;
    let task = task(&t, &alice, "量尺寸", Some(&home)).await;
    let a = t.attach(&alice, &task, "户型图.txt", b"plan").await;
    assert_eq!(names(&t.snapshot(&bob).await), ["户型图.txt"]);
    assert_eq!(t.download(&bob, &a).await.0, StatusCode::OK);
    assert_eq!(t.download(&carol, &a).await.0, StatusCode::NOT_FOUND);

    // Bob deleting what Alice uploaded can undo it.
    let deleted = id();
    mutate_as(&t, &bob, &deleted, json!([{ "op": "attachment.delete", "id": a }])).await;
    assert_eq!(undo(&t, &bob, &deleted).await.status, StatusCode::OK);
    assert_eq!(names(&t.snapshot(&alice).await), ["户型图.txt"]);

    // Out of the shared folder, it goes from Bob's data with the task; back in, it comes back.
    let since = t.snapshot(&bob).await["seq"].as_i64().unwrap();
    t.mutate(&alice, json!([{ "op": "task.update", "id": task, "folder_id": private }])).await;
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&bob), None).await.body;
    let effects = &changes["changes"][0]["effects"];
    assert!(effects.as_array().unwrap().iter().any(|e| e["kind"] == "attachment" && e["before"]["id"] == a && e["after"].is_null()));
    assert!(names(&t.snapshot(&bob).await).is_empty());
    assert_eq!(t.download(&bob, &a).await.0, StatusCode::NOT_FOUND);
    assert_eq!(names(&t.snapshot(&alice).await), ["户型图.txt"]);

    let since = t.snapshot(&bob).await["seq"].as_i64().unwrap();
    t.ok(&alice, "move", json!({ "folders": ["/私人"], "to": "/装修" })).await;
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&bob), None).await.body;
    let effects = &changes["changes"][0]["effects"];
    assert!(effects.as_array().unwrap().iter().any(|e| e["kind"] == "attachment" && e["before"].is_null() && e["after"]["id"] == a));
    assert_eq!(names(&t.snapshot(&bob).await), ["户型图.txt"]);
}

#[tokio::test]
async fn unused_files_go_after_ninety_days_and_what_was_left_behind_after_a_day() {
    let t = TestApp::new();
    let s = t.admin().await;
    let task = task(&t, &s, "报价", None).await;
    let unattached = id();
    t.upload(&s, &unattached, "text/plain", b"x").await;
    let detached = t.attach(&s, &task, "a.txt", b"hello").await;
    t.mutate(&s, json!([{ "op": "attachment.delete", "id": detached }])).await;
    let kept = t.attach(&s, &task, "b.txt", b"kept").await;

    let dir = &t.files;
    let stray = |name: &str, age: u64| {
        let path = dir.join(name);
        std::fs::write(&path, "left").unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
        path
    };
    let (old, part, new) = (stray(&id(), 2 * 86_400), stray(".part-x", 2 * 86_400), stray(&id(), 60));

    let clear = |days: i64| {
        let (db, files, now) = (t.state.db.clone(), t.state.files.clone(), t.state.clock.now() + days * DAY);
        async move { db.call(move |c| attachments::clear(c, &files, now)).await.unwrap() }
    };
    assert_eq!(clear(89).await, (0, 2));
    assert!(!old.exists() && !part.exists() && new.exists());
    assert_eq!(t.download(&s, &unattached).await.0, StatusCode::OK);

    assert_eq!(clear(91).await, (2, 0));
    assert_eq!(t.download(&s, &unattached).await.0, StatusCode::NOT_FOUND);
    assert!(!dir.join(&detached).exists());
    assert_eq!(t.download(&s, &kept).await.2, b"kept");
    // Gone, the attachment can't be brought back; its file is to be uploaded again.
    let r = t
        .mutate(&s, json!([{ "op": "attachment.add", "id": unattached, "name": "x", "type": "text/plain", "size": 1, "task_id": task }]))
        .await;
    assert_eq!(r.body["code"], "file_missing");
}

#[tokio::test]
async fn agents_attach_read_rename_and_detach() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "设计稿" })).await;
    let png = b"\x89PNG not really";
    let r = t.ok(&s, "attach", json!({ "task": 1, "name": "草图.png", "data": BASE64.encode(png) })).await;
    assert!(r["text"].as_str().unwrap().contains("添加附件 草图.png（#1）"), "{}", r["text"]);
    t.ok(&s, "attach", json!({ "task": 1, "name": "说明.txt", "data": BASE64.encode("先画草图".as_bytes()) })).await;

    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(shown["text"].as_str().unwrap().contains("草图.png（image/png，15 B）"), "{}", shown["text"]);
    let list = shown["data"]["attachments"].as_array().unwrap();
    let (image, note) = (list[0]["id"].as_str().unwrap(), list[1]["id"].as_str().unwrap());

    let read = t.ok(&s, "attachment", json!({ "id": image })).await;
    assert_eq!(BASE64.decode(read["data"]["image"].as_str().unwrap()).unwrap(), png);
    let read = t.ok(&s, "attachment", json!({ "id": note })).await;
    assert_eq!(read["data"]["text"], "先画草图");
    assert!(read["text"].as_str().unwrap().ends_with("先画草图"));

    t.ok(&s, "rename", json!({ "attachment": image, "name": "终稿.png" })).await;
    assert_eq!(names(&t.snapshot(&s).await), ["终稿.png", "说明.txt"]);
    assert_eq!(t.tool(&s, "rename", json!({ "attachment": image, "name": "a/b" })).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(t.tool(&s, "rename", json!({ "name": "x" })).await.status, StatusCode::BAD_REQUEST);

    t.ok(&s, "detach", json!({ "attachments": [image, note] })).await;
    assert!(names(&t.snapshot(&s).await).is_empty());
    assert_eq!(t.tool(&s, "attachment", json!({ "id": image })).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_android_page_may_upload_and_download() {
    let t = TestApp::new();
    let path = format!("/api/v1/files/{}", id());
    let preflight = Request::builder()
        .method("OPTIONS")
        .uri(&path)
        .header(header::ORIGIN, "https://localhost")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PUT")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization,content-type");
    // Without signing in: the browser asks first, with no token.
    let (status, h, _) = t.raw(preflight, Body::empty()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h[header::ACCESS_CONTROL_ALLOW_ORIGIN], "https://localhost");
    assert!(h[header::ACCESS_CONTROL_ALLOW_METHODS].to_str().unwrap().contains("PUT"));
}

#[tokio::test]
async fn a_file_copies_from_an_attachment_one_sees() {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    let plan = task(&t, &alice, "合同", None).await;
    let original = t.attach(&alice, &plan, "合同.txt", b"terms").await;
    let copy = |s: &str, from: &str| {
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/files/{}?from={from}", id()))
            .header(header::AUTHORIZATION, format!("Bearer {s}"));
        let t = &t;
        async move { t.raw(req, Body::empty()).await }
    };

    let new = id();
    let copy_to = |s: &str, to: &str| {
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/files/{to}?from={original}"))
            .header(header::AUTHORIZATION, format!("Bearer {s}"));
        let t = &t;
        async move { t.raw(req, Body::empty()).await }
    };
    let (status, _, body) = copy_to(&alice, &new).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["size"], 5);
    let op = json!([{ "op": "attachment.add", "id": new, "name": "合同.txt", "type": "text/plain", "size": 5, "task_id": plan }]);
    assert_eq!(t.mutate(&alice, op).await.status, StatusCode::OK, "attached as an upload is");
    assert_eq!(t.download(&alice, &new).await.2, b"terms");

    assert_eq!(copy(&bob, &original).await.0, StatusCode::NOT_FOUND, "not his to see");
    assert_eq!(copy(&alice, &id()).await.0, StatusCode::NOT_FOUND);
    // Sent again after the reply was lost, it gets the first answer, the original gone since or not.
    assert_eq!(t.mutate(&alice, json!([{ "op": "attachment.delete", "id": original }])).await.status, StatusCode::OK);
    assert_eq!(copy_to(&alice, &new).await.0, StatusCode::OK);
}
