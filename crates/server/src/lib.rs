//! NextTodo server: the HTTP API over SQLite. See `docs/api.md`.

#[cfg(feature = "agent-port")]
mod agent_port;
pub mod ai;
pub mod auth;
pub mod backup;
pub mod db;
pub mod error;
pub mod local;
pub mod mcp;
pub mod mutate;
pub mod start;
pub mod store;
pub mod sync;
pub mod tools;
pub mod upgrade;
pub mod ws;

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, Uri, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use tower::ServiceExt;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

pub use db::Db;
use error::ApiError;

/// How long the mutation log is kept.
pub const MUTATION_RETENTION_MS: i64 = 90 * 24 * 3600 * 1000;

/// Milliseconds since the Unix epoch; replaceable so that tests control time.
#[derive(Clone)]
pub struct Clock(pub Arc<dyn Fn() -> i64 + Send + Sync>);

impl Clock {
    pub fn system() -> Self {
        Clock(Arc::new(|| {
            let ms = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after 1970").as_millis();
            i64::try_from(ms).expect("milliseconds fit in i64")
        }))
    }

    pub fn now(&self) -> i64 {
        (self.0)()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub clock: Clock,
    pub hub: Hub,
    pub ai: Arc<ai::Ai>,
    /// Only where the server can upgrade itself.
    pub upgrade: Option<Arc<upgrade::Upgrade>>,
}

impl AppState {
    /// With API keys kept only on devices, and models called over HTTPS.
    pub fn new(db: Db, clock: Clock) -> Self {
        AppState {
            db,
            clock,
            hub: Hub::default(),
            ai: Arc::new(ai::Ai::new(None, Arc::new(ai::llm::Https { public_only: true }))),
            upgrade: None,
        }
    }

    pub fn with_ai(self, ai: ai::Ai) -> Self {
        AppState { ai: Arc::new(ai), ..self }
    }

    pub fn with_upgrade(self, upgrade: Arc<upgrade::Upgrade>) -> Self {
        AppState { upgrade: Some(upgrade), ..self }
    }
}

/// Tells open sync connections that a user has new changes. It carries only the user id: each
/// connection reads what it has not sent yet from the database, so nothing is missed or reordered.
#[derive(Clone)]
pub struct Hub(tokio::sync::broadcast::Sender<Arc<str>>);

impl Default for Hub {
    fn default() -> Self {
        Hub(tokio::sync::broadcast::channel(256).0)
    }
}

impl Hub {
    pub fn changed(&self, owner: &str) {
        // An error only means no connection is listening.
        let _ = self.0.send(owner.into());
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Arc<str>> {
        self.0.subscribe()
    }
}

pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me).patch(auth::update_me))
        .route("/auth/password", post(auth::change_password))
        .route("/tokens", get(auth::list_tokens).post(auth::create_token))
        .route("/tokens/{id}", delete(auth::delete_token))
        .route("/invites", get(auth::list_invites).post(auth::create_invite))
        .route("/admin/upgrade", get(upgrade::status).post(upgrade::start))
        .route("/snapshot", get(sync::snapshot))
        .route("/changes", get(sync::changes))
        .route("/mutations", post(sync::mutate))
        .route("/sync", get(ws::sync))
        .route("/tools", get(tools::list_tools))
        .route("/tools/{name}", post(tools::call_tool))
        .route("/ai/providers", get(ai::providers))
        .route("/ai/settings", get(ai::get_settings).put(ai::put_settings).delete(ai::delete_settings))
        .route("/ai/test", post(ai::test))
        .route("/ai/models", post(ai::models))
        .route("/ai/jobs/{id}/run", post(ai::run))
        .fallback(|| async { ApiError::not_found("接口不存在") });
    let mcp = Router::new()
        .route_service("/mcp", mcp::service(state.clone()))
        .route_layer(middleware::from_fn_with_state(state.clone(), mcp::authenticate));
    Router::new().nest("/api/v1", api).merge(mcp).layer(CompressionLayer::new()).layer(TraceLayer::new_for_http()).with_state(state)
}

/// Adds the web app's files from `dir`, using the compressed copies made at build time. Page
/// addresses (no file extension in the last segment) get `index.html`, so the app's own routes work
/// when opened directly; a missing file is a 404, and unknown `/api` and `/mcp` paths answer in JSON.
pub fn with_web(router: Router, dir: &Path) -> Router {
    let files = ServeDir::new(dir).precompressed_br().precompressed_gzip();
    router.fallback(move |req: Request| web(files.clone(), req))
}

async fn web(files: ServeDir, mut req: Request) -> Response {
    let path = req.uri().path();
    if path == "/api" || path.starts_with("/api/") || path.starts_with("/mcp/") {
        return ApiError::not_found("接口不存在").into_response();
    }
    // Built assets have the content hash in their name and never change; everything else is
    // checked on every load so that a new version shows up.
    let immutable = path.starts_with("/assets/");
    if !path.rsplit('/').next().is_some_and(|last| last.contains('.')) {
        *req.uri_mut() = Uri::from_static("/index.html");
    }
    let Ok(res) = files.oneshot(req).await;
    let mut res = res.map(Body::new);
    let cache = if immutable && res.status().is_success() { "public, max-age=31536000, immutable" } else { "no-cache" };
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}
