//! A test server over an in-memory database, with a clock the test controls.

#![allow(dead_code, reason = "each test file uses a different part of this")]

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use nexttodo_server::ai::Ai;
use nexttodo_server::files::Files;
use nexttodo_server::upgrade::Upgrade;
use nexttodo_server::{AppState, Clock, Db, app, auth};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

pub struct TestApp {
    pub app: Router,
    pub state: AppState,
    time: Arc<AtomicI64>,
    /// Where attachments' files go, deleted with the app.
    pub files: std::path::PathBuf,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.files);
    }
}

pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
    pub headers: HeaderMap,
}

pub fn id() -> String {
    Uuid::now_v7().to_string()
}

impl TestApp {
    pub fn new() -> Self {
        Self::on(Db::open_in_memory().unwrap())
    }

    /// Over `db`, a file say.
    pub fn on(db: Db) -> Self {
        Self::build(db, |s| s)
    }

    pub fn with_ai(ai: Ai) -> Self {
        Self::build(Db::open_in_memory().unwrap(), |s| s.with_ai(ai))
    }

    pub fn with_upgrade(upgrade: Arc<Upgrade>) -> Self {
        Self::build(Db::open_in_memory().unwrap(), |s| s.with_upgrade(upgrade))
    }

    fn build(db: Db, f: impl FnOnce(AppState) -> AppState) -> Self {
        let time = Arc::new(AtomicI64::new(1_762_000_000_000));
        let t = time.clone();
        let files = std::env::temp_dir().join(format!("nexttodo-files-{}", Uuid::now_v7()));
        let state = f(AppState::new(db, Clock(Arc::new(move || t.load(Ordering::SeqCst))), Files::new(files.clone())));
        TestApp { app: app(state.clone()), state, time, files }
    }

    pub fn advance(&self, ms: i64) {
        self.time.fetch_add(ms, Ordering::SeqCst);
    }

    pub async fn call(&self, method: &str, path: &str, auth: Option<&str>, body: Option<Value>) -> Reply {
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

    pub async fn login(&self, username: &str, password: &str) -> Reply {
        self.call("POST", "/auth/login", None, Some(json!({ "username": username, "password": password, "device": "测试" }))).await
    }

    /// Creates the administrator and returns a session secret.
    pub async fn admin(&self) -> String {
        auth::create_admin(&self.state, "alice".into(), "Alice".into(), "correct horse".into()).await.unwrap();
        self.login("alice", "correct horse").await.body["secret"].as_str().unwrap().to_owned()
    }

    /// The id of the user a session belongs to.
    pub async fn me(&self, secret: &str) -> String {
        self.call("GET", "/me", Some(secret), None).await.body["id"].as_str().unwrap().to_owned()
    }

    /// Registers a regular user through an invite from `admin` and returns a session secret.
    pub async fn user(&self, admin: &str, username: &str) -> String {
        let code = self.call("POST", "/invites", Some(admin), None).await.body["code"].as_str().unwrap().to_owned();
        let reg = json!({ "invite": code, "username": username, "password": "password123", "display_name": username, "device": "手机" });
        self.call("POST", "/auth/register", None, Some(reg)).await.body["secret"].as_str().unwrap().to_owned()
    }

    pub async fn mutate(&self, secret: &str, ops: Value) -> Reply {
        self.call("POST", "/mutations", Some(secret), Some(json!({ "id": id(), "ops": ops }))).await
    }

    /// Calls a tool and returns its reply; on success the body is `{text, data}`.
    pub async fn tool(&self, secret: &str, name: &str, args: Value) -> Reply {
        self.call("POST", &format!("/tools/{name}"), Some(secret), Some(args)).await
    }

    /// Calls a tool that must succeed and returns its `{text, data}`.
    pub async fn ok(&self, secret: &str, name: &str, args: Value) -> Value {
        let r = self.tool(secret, name, args).await;
        assert_eq!(r.status, StatusCode::OK, "{name}: {}", r.body);
        r.body
    }

    /// A request with `body` as it is, answered as it is.
    pub async fn raw(&self, req: axum::http::request::Builder, body: Body) -> (StatusCode, HeaderMap, Vec<u8>) {
        let res = self.app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        (status, headers, res.into_body().collect().await.unwrap().to_bytes().to_vec())
    }

    /// Uploads `bytes` as attachment `id`'s file.
    pub async fn upload(&self, secret: &str, id: &str, media_type: &str, bytes: &[u8]) -> Reply {
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/files/{id}"))
            .header(header::AUTHORIZATION, format!("Bearer {secret}"))
            .header(header::CONTENT_TYPE, media_type);
        let (status, headers, bytes) = self.raw(req, Body::from(bytes.to_vec())).await;
        Reply { status, headers, body: if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() } }
    }

    pub async fn download(&self, secret: &str, id: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        let req = Request::builder().uri(format!("/api/v1/files/{id}")).header(header::AUTHORIZATION, format!("Bearer {secret}"));
        self.raw(req, Body::empty()).await
    }

    /// Uploads a file and attaches it to task `task` as `name`, returning the attachment's id.
    pub async fn attach(&self, secret: &str, task: &str, name: &str, bytes: &[u8]) -> String {
        let a = id();
        let up = self.upload(secret, &a, "text/plain", bytes).await;
        assert_eq!(up.status, StatusCode::OK, "{}", up.body);
        let op = json!([{ "op": "attachment.add", "id": a, "name": name, "type": "text/plain", "size": bytes.len(), "task_id": task }]);
        let r = self.mutate(secret, op).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        a
    }

    pub async fn snapshot(&self, secret: &str) -> Value {
        self.call("GET", "/snapshot", Some(secret), None).await.body
    }
}
