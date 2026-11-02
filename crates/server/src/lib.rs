//! NextTodo server: the HTTP API over SQLite. See `docs/api.md`.

pub mod auth;
pub mod db;
pub mod error;
pub mod mcp;
pub mod mutate;
pub mod store;
pub mod sync;
pub mod tools;
pub mod ws;

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
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
}

impl AppState {
    pub fn new(db: Db, clock: Clock) -> Self {
        AppState { db, clock, hub: Hub::default() }
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
        .route("/snapshot", get(sync::snapshot))
        .route("/changes", get(sync::changes))
        .route("/mutations", post(sync::mutate))
        .route("/sync", get(ws::sync))
        .route("/tools", get(tools::list_tools))
        .route("/tools/{name}", post(tools::call_tool))
        .fallback(|| async { ApiError::not_found("接口不存在") });
    let mcp = Router::new()
        .route_service("/mcp", mcp::service(state.clone()))
        .route_layer(middleware::from_fn_with_state(state.clone(), mcp::authenticate));
    Router::new().nest("/api/v1", api).merge(mcp).layer(CompressionLayer::new()).layer(TraceLayer::new_for_http()).with_state(state)
}

/// Adds the web app's files from `dir`, using the compressed copies made at build time. Paths that
/// match no file get `index.html`, so the app's own routes work when opened directly.
pub fn with_web(router: Router, dir: &Path) -> Router {
    let index = ServeFile::new(dir.join("index.html")).precompressed_br().precompressed_gzip();
    let files = ServeDir::new(dir).precompressed_br().precompressed_gzip().fallback(index);
    router.fallback_service(Router::new().fallback_service(files).layer(middleware::from_fn(cache_control)))
}

/// Built assets have the content hash in their name and never change; everything else is checked
/// on every load so that a new version shows up.
async fn cache_control(req: Request, next: Next) -> Response {
    let immutable = req.uri().path().starts_with("/assets/");
    let mut res = next.run(req).await;
    let value = if immutable { "public, max-age=31536000, immutable" } else { "no-cache" };
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}
