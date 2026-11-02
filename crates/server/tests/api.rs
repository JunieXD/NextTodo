use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use nexttodo_server::{AppState, Clock, Db, MUTATION_RETENTION_MS, app, auth, db};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

struct TestApp {
    app: Router,
    state: AppState,
    time: Arc<AtomicI64>,
}

struct Reply {
    status: StatusCode,
    body: Value,
    headers: HeaderMap,
}

fn id() -> String {
    Uuid::now_v7().to_string()
}

impl TestApp {
    fn new() -> Self {
        let time = Arc::new(AtomicI64::new(1_762_000_000_000));
        let t = time.clone();
        let state = AppState { db: Db::open_in_memory().unwrap(), clock: Clock(Arc::new(move || t.load(Ordering::SeqCst))) };
        TestApp { app: app(state.clone()), state, time }
    }

    fn advance(&self, ms: i64) {
        self.time.fetch_add(ms, Ordering::SeqCst);
    }

    async fn call(&self, method: &str, path: &str, auth: Option<&str>, body: Option<Value>) -> Reply {
        let mut req = Request::builder().method(method).uri(format!("/api/v1{path}"));
        if let Some(secret) = auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {secret}"));
        }
        let req =
            req.header(header::CONTENT_TYPE, "application/json").body(body.map_or(Body::empty(), |b| Body::from(b.to_string()))).unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        Reply { status, headers, body: if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() } }
    }

    async fn login(&self, username: &str, password: &str) -> Reply {
        self.call("POST", "/auth/login", None, Some(json!({ "username": username, "password": password, "device": "测试" }))).await
    }

    /// Creates the administrator and returns a session secret.
    async fn admin(&self) -> String {
        auth::create_admin(&self.state, "junie".into(), "Junie".into(), "correct horse".into()).await.unwrap();
        self.login("junie", "correct horse").await.body["secret"].as_str().unwrap().to_owned()
    }

    /// Registers a regular user through an invite from `admin` and returns a session secret.
    async fn user(&self, admin: &str, username: &str) -> String {
        let code = self.call("POST", "/invites", Some(admin), None).await.body["code"].as_str().unwrap().to_owned();
        let reg = json!({ "invite": code, "username": username, "password": "password123", "display_name": username, "device": "手机" });
        self.call("POST", "/auth/register", None, Some(reg)).await.body["secret"].as_str().unwrap().to_owned()
    }

    async fn mutate(&self, secret: &str, ops: Value) -> Reply {
        self.call("POST", "/mutations", Some(secret), Some(json!({ "id": id(), "ops": ops }))).await
    }

    async fn snapshot(&self, secret: &str) -> Value {
        self.call("GET", "/snapshot", Some(secret), None).await.body
    }
}

#[tokio::test]
async fn login_sessions_and_logout() {
    let t = TestApp::new();
    let admin = t.admin().await;
    assert_eq!(t.login("junie", "wrong password").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.login("nobody", "correct horse").await.status, StatusCode::UNAUTHORIZED);

    let login = t.login("JUNIE", "correct horse").await;
    assert_eq!(login.status, StatusCode::OK, "usernames are case-insensitive");
    let cookie = login.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("nt_session=nt_") && cookie.contains("HttpOnly") && cookie.contains("Secure"));

    let me = t.call("GET", "/me", Some(&admin), None).await;
    assert_eq!(me.body["username"], "junie");
    assert_eq!(me.body["is_admin"], true);

    let via_cookie =
        Request::builder().uri("/api/v1/me").header(header::COOKIE, cookie.split(';').next().unwrap()).body(Body::empty()).unwrap();
    assert_eq!(t.app.clone().oneshot(via_cookie).await.unwrap().status(), StatusCode::OK);

    assert_eq!(t.call("POST", "/auth/logout", Some(&admin), None).await.status, StatusCode::NO_CONTENT);
    let after = t.call("GET", "/me", Some(&admin), None).await;
    assert_eq!((after.status, after.body["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("unauthenticated")));
}

#[tokio::test]
async fn invites_registration_and_token_permissions() {
    let t = TestApp::new();
    let admin = t.admin().await;
    let code = t.call("POST", "/invites", Some(&admin), None).await.body["code"].as_str().unwrap().to_owned();
    let reg = json!({ "invite": code, "username": "friend", "password": "password123", "display_name": "朋友", "device": "手机" });
    let first = t.call("POST", "/auth/register", None, Some(reg.clone())).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(t.call("POST", "/auth/register", None, Some(reg)).await.status, StatusCode::CONFLICT, "an invite works once");
    let invites = t.call("GET", "/invites", Some(&admin), None).await.body;
    assert_eq!(invites[0]["used_by"], "friend");

    let friend = first.body["secret"].as_str().unwrap();
    assert_eq!(t.call("POST", "/invites", Some(friend), None).await.status, StatusCode::FORBIDDEN, "only admins invite");

    let made = t.call("POST", "/tokens", Some(&admin), Some(json!({ "name": "Claude Code", "scope": "read" }))).await.body;
    let read_only = made["secret"].as_str().unwrap();
    assert_eq!(t.call("GET", "/snapshot", Some(read_only), None).await.status, StatusCode::OK);
    let denied = t.mutate(read_only, json!([{ "op": "folder.create", "id": id(), "name": "工作" }])).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let minted = t.call("POST", "/tokens", Some(read_only), Some(json!({ "name": "x", "scope": "write" }))).await;
    assert_eq!(minted.status, StatusCode::FORBIDDEN, "API tokens cannot mint tokens");

    let tokens = t.call("GET", "/tokens", Some(&admin), None).await.body;
    assert_eq!(tokens.as_array().unwrap().len(), 2);
    let token_id = made["token"]["id"].as_str().unwrap();
    assert_eq!(t.call("DELETE", &format!("/tokens/{token_id}"), Some(&admin), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(t.call("GET", "/snapshot", Some(read_only), None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn one_mutation_creates_folders_tasks_and_dependencies() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (folder, a, b) = (id(), id(), id());
    let r = t
        .mutate(
            &s,
            json!([
                { "op": "folder.create", "id": folder, "name": "毕业论文" },
                { "op": "task.create", "id": a, "folder_id": folder, "title": "设计问卷", "due": { "date": "2026-11-02", "time": "18:00", "remind_before": 60 } },
                { "op": "task.create", "id": b, "folder_id": folder, "title": "发放问卷" },
                { "op": "dep.add", "before": a, "after": b }
            ]),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["effects"].as_array().unwrap().len(), 4);

    let snap = t.snapshot(&s).await;
    assert_eq!(snap["seq"], r.body["seq"]);
    let tasks = snap["tasks"].as_array().unwrap();
    assert_eq!((tasks[0]["number"].as_i64(), tasks[1]["number"].as_i64()), (Some(1), Some(2)));
    assert_eq!((tasks[0]["sort_key"].as_str(), tasks[1]["sort_key"].as_str()), (Some("a0"), Some("a1")));
    assert_eq!(tasks[0]["due"], json!({ "date": "2026-11-02", "time": "18:00", "remind_before": 60 }));
    assert_eq!(tasks[0]["created_via"], Value::Null);
    assert_eq!(snap["dependencies"], json!([{ "before": a, "after": b, "created_at": tasks[0]["created_at"] }]));
}

#[tokio::test]
async fn cycles_are_rejected_and_nothing_is_applied() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (a, b, c) = (id(), id(), id());
    t.mutate(
        &s,
        json!([
            { "op": "task.create", "id": a, "title": "A" },
            { "op": "task.create", "id": b, "title": "B" },
            { "op": "task.create", "id": c, "title": "C" },
            { "op": "dep.add", "before": a, "after": b },
            { "op": "dep.add", "before": b, "after": c }
        ]),
    )
    .await;
    let before = t.snapshot(&s).await;
    let r = t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "D" }, { "op": "dep.add", "before": c, "after": a }])).await;
    assert_eq!((r.status, r.body["code"].as_str()), (StatusCode::CONFLICT, Some("cycle")));
    assert_eq!(r.body["details"]["path"], json!([c, a, b, c]));
    assert_eq!(t.snapshot(&s).await, before, "the whole mutation is rolled back");

    let dup = t.mutate(&s, json!([{ "op": "dep.add", "before": a, "after": b }])).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    let own = t.mutate(&s, json!([{ "op": "dep.add", "before": a, "after": a }])).await;
    assert_eq!(own.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn folders_cannot_move_into_their_own_subtree() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (top, mid, leaf) = (id(), id(), id());
    t.mutate(
        &s,
        json!([
            { "op": "folder.create", "id": top, "name": "学习" },
            { "op": "folder.create", "id": mid, "name": "毕业论文", "parent_id": top },
            { "op": "folder.create", "id": leaf, "name": "答辩", "parent_id": mid }
        ]),
    )
    .await;
    let r = t.mutate(&s, json!([{ "op": "folder.update", "id": top, "parent_id": leaf }])).await;
    assert_eq!(r.body["code"], "folder_cycle");
    let to_root = t.mutate(&s, json!([{ "op": "folder.update", "id": leaf, "parent_id": null }])).await;
    assert_eq!(to_root.status, StatusCode::OK);
    let folders = t.snapshot(&s).await["folders"].clone();
    assert!(folders.as_array().unwrap().iter().any(|f| f["id"] == leaf.as_str() && f["parent_id"].is_null()));
}

#[tokio::test]
async fn deleting_a_folder_can_be_undone_completely() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (top, sub, a, b, outside) = (id(), id(), id(), id(), id());
    t.mutate(
        &s,
        json!([
            { "op": "folder.create", "id": top, "name": "毕业论文" },
            { "op": "folder.create", "id": sub, "name": "答辩", "parent_id": top },
            { "op": "task.create", "id": a, "folder_id": top, "title": "写初稿", "description": "- [ ] 引言" },
            { "op": "task.create", "id": b, "folder_id": sub, "title": "做 PPT" },
            { "op": "task.create", "id": outside, "title": "交电费" },
            { "op": "dep.add", "before": a, "after": b },
            { "op": "dep.add", "before": outside, "after": a }
        ]),
    )
    .await;
    let before = t.snapshot(&s).await;
    let delete_id = id();
    let deleted =
        t.call("POST", "/mutations", Some(&s), Some(json!({ "id": delete_id, "ops": [{ "op": "folder.delete", "id": top }] }))).await;
    assert_eq!(deleted.body["effects"].as_array().unwrap().len(), 6, "2 dependencies, 2 tasks, 2 folders");
    let after = t.snapshot(&s).await;
    assert_eq!((after["folders"].as_array().unwrap().len(), after["tasks"].as_array().unwrap().len()), (0, 1));
    assert_eq!(after["dependencies"], json!([]));

    t.advance(1000);
    let undo = t.mutate(&s, json!([{ "op": "undo", "mutation": delete_id }])).await;
    assert_eq!(undo.status, StatusCode::OK, "{}", undo.body);
    let restored = t.snapshot(&s).await;
    let strip = |v: &Value| {
        let mut v = v.clone();
        for list in ["folders", "tasks"] {
            for e in v[list].as_array_mut().unwrap() {
                e.as_object_mut().unwrap().remove("updated_at");
            }
        }
        v.as_object_mut().unwrap().remove("seq");
        v
    };
    assert_eq!(strip(&restored), strip(&before), "same ids, numbers, content and dependencies");

    let again = t.mutate(&s, json!([{ "op": "undo", "mutation": delete_id }])).await;
    assert_eq!(again.body["effects"], json!([]), "undoing twice changes nothing more");
}

#[tokio::test]
async fn updates_completion_and_undo_of_an_update() {
    let t = TestApp::new();
    let s = t.admin().await;
    let a = id();
    t.mutate(&s, json!([{ "op": "task.create", "id": a, "title": "设计问卷", "due": { "date": "2026-11-20" } }])).await;
    t.advance(5000);
    let update_id = id();
    let ops = json!([{ "op": "task.update", "id": a, "title": "设计问卷（第二版）", "done": true, "due": null }]);
    let r = t.call("POST", "/mutations", Some(&s), Some(json!({ "id": update_id, "ops": ops }))).await;
    let task = &r.body["effects"][0]["after"];
    assert_eq!((task["title"].as_str(), task["due"].is_null()), (Some("设计问卷（第二版）"), true));
    assert_eq!(task["done_at"], t.state.clock.now());

    let same = t.mutate(&s, json!([{ "op": "task.update", "id": a, "done": true }])).await;
    assert_eq!(same.body["effects"], json!([]), "completing a completed task changes nothing");

    t.mutate(&s, json!([{ "op": "undo", "mutation": update_id }])).await;
    let task = &t.snapshot(&s).await["tasks"][0];
    assert_eq!((task["title"].as_str(), task["done_at"].is_null()), (Some("设计问卷"), true));
    assert_eq!(task["due"], json!({ "date": "2026-11-20" }));
}

#[tokio::test]
async fn resubmitting_a_mutation_applies_it_once() {
    let t = TestApp::new();
    let s = t.admin().await;
    let body = json!({ "id": id(), "ops": [{ "op": "task.create", "id": id(), "title": "交电费" }] });
    let first = t.call("POST", "/mutations", Some(&s), Some(body.clone())).await;
    let second = t.call("POST", "/mutations", Some(&s), Some(body)).await;
    assert_eq!(first.body, second.body);
    assert_eq!(t.snapshot(&s).await["tasks"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn changes_since_and_resync_after_pruning() {
    let t = TestApp::new();
    let s = t.admin().await;
    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "Claude Code", "scope": "write" }))).await.body;
    let agent = token["secret"].as_str().unwrap();
    let first = t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "一" }])).await.body["seq"].as_i64().unwrap();
    t.mutate(agent, json!([{ "op": "task.create", "id": id(), "title": "二" }])).await;

    let all = t.call("GET", "/changes?since=0", Some(&s), None).await.body;
    assert_eq!(all["changes"].as_array().unwrap().len(), 2);
    assert_eq!((all["changes"][0]["actor"].clone(), all["changes"][1]["actor"].as_str()), (Value::Null, Some("Claude Code")));
    assert_eq!(all["changes"][1]["effects"][0]["after"]["created_via"], "Claude Code");
    let newer = t.call("GET", &format!("/changes?since={first}"), Some(&s), None).await.body;
    assert_eq!(newer["changes"].as_array().unwrap().len(), 1);
    assert_eq!(newer["seq"], all["seq"]);

    t.advance(MUTATION_RETENTION_MS + 1);
    let cutoff = t.state.clock.now() - MUTATION_RETENTION_MS;
    t.state.db.call(move |c| Ok(db::prune(c, cutoff)?)).await.unwrap();
    let stale = t.call("GET", "/changes?since=0", Some(&s), None).await;
    assert_eq!((stale.status, stale.body["code"].as_str()), (StatusCode::GONE, Some("resync")));
    let current = t.call("GET", &format!("/changes?since={}", all["seq"]), Some(&s), None).await;
    assert_eq!((current.status, current.body["changes"].clone()), (StatusCode::OK, json!([])));
    assert_eq!(t.snapshot(&s).await["seq"], all["seq"], "the latest seq survives pruning");
}

#[tokio::test]
async fn users_cannot_see_or_touch_each_others_data() {
    let t = TestApp::new();
    let admin = t.admin().await;
    let friend = t.user(&admin, "friend").await;
    let (task, folder) = (id(), id());
    t.mutate(
        &admin,
        json!([{ "op": "folder.create", "id": folder, "name": "私人" }, { "op": "task.create", "id": task, "title": "秘密" }]),
    )
    .await;

    let snap = t.snapshot(&friend).await;
    assert_eq!((snap["tasks"].clone(), snap["folders"].clone()), (json!([]), json!([])));
    for ops in [
        json!([{ "op": "task.update", "id": task, "title": "改掉" }]),
        json!([{ "op": "task.create", "id": id(), "title": "x", "folder_id": folder }]),
        json!([{ "op": "task.create", "id": id(), "title": "x" }, { "op": "dep.add", "before": task, "after": task }]),
    ] {
        let r = t.mutate(&friend, ops).await;
        assert!(matches!(r.status, StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST), "{}", r.body);
    }
    let reuse = t.mutate(&friend, json!([{ "op": "task.create", "id": task, "title": "抢 ID" }])).await;
    assert_eq!(reuse.status, StatusCode::CONFLICT);
    assert_eq!(t.snapshot(&admin).await["tasks"][0]["title"], "秘密");
}

#[tokio::test]
async fn bad_input_names_the_field() {
    let t = TestApp::new();
    let s = t.admin().await;
    let r = t
        .mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "x", "due": { "date": "2026-11-20", "remind_before": 10 } }]))
        .await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("ops[0]")));
    assert_eq!(r.body["message"], "只有设了具体的截止时间才能设提醒");
    let r = t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "   " }])).await;
    assert_eq!((r.body["details"]["field"].as_str(), r.body["message"].as_str()), (Some("ops[0]"), Some("标题不能为空")));
    let r = t.call("GET", "/changes?since=abc", Some(&s), None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = t.call("GET", "/nope", Some(&s), None).await;
    assert_eq!(r.body["code"], "not_found");
}
