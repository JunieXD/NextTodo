//! NextTodo server: the HTTP API over SQLite. See `docs/api.md`.

#[cfg(feature = "agent-port")]
mod agent_port;
pub mod ai;
pub mod attachments;
pub mod auth;
pub mod backup;
pub mod db;
pub mod drive;
pub mod error;
pub mod files;
pub mod local;
pub mod mcp;
pub mod mutate;
pub mod start;
pub mod store;
pub mod sync;
pub mod tools;
pub mod upgrade;
pub mod ws;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::extract::Request;
use axum::http::{HeaderValue, Uri, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use files::Files;
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
    pub files: Arc<Files>,
    /// In the app, for its one user, with its data directory (docs/local.md).
    pub local: Option<PathBuf>,
    /// Syncing through a drive, every change to the shared data goes through it (docs/local.md#借网盘同步).
    pub drive: Arc<drive::Syncing>,
}

impl AppState {
    /// With API keys kept only on devices, and models called over HTTPS.
    pub fn new(db: Db, clock: Clock, files: Files) -> Self {
        AppState {
            db,
            clock,
            hub: Hub::default(),
            ai: Arc::new(ai::Ai::new(None, Arc::new(ai::llm::Https { public_only: true }))),
            upgrade: None,
            files: Arc::new(files),
            local: None,
            drive: Arc::default(),
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
/// Called from inside the database call that commits the change, so a connection reads after it.
#[derive(Clone)]
pub struct Hub(tokio::sync::broadcast::Sender<Arc<str>>);

impl Default for Hub {
    fn default() -> Self {
        Hub(tokio::sync::broadcast::channel(256).0)
    }
}

impl Hub {
    pub fn changed(&self, user: &str) {
        // An error only means no connection is listening.
        let _ = self.0.send(user.into());
    }

    /// Tells everyone a change went to.
    pub fn notify(&self, users: &[String]) {
        for user in users {
            self.changed(user);
        }
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
        .route("/files/{id}", get(attachments::get_file).put(attachments::put_file).layer(page_cors()))
        .route("/settings", get(attachments::get_settings).put(attachments::put_settings))
        .route("/tools/{name}", post(tools::call_tool).layer(DefaultBodyLimit::max(tools::MAX_BODY)))
        .route("/ai/providers", get(ai::providers))
        .route("/ai/settings", get(ai::get_settings).put(ai::put_settings).delete(ai::delete_settings))
        .route("/ai/test", post(ai::test))
        .route("/ai/models", post(ai::models))
        .route("/ai/jobs/{id}/run", post(ai::run))
        .route("/ai/notes", get(ai::get_notes).put(ai::put_note))
        .route("/local/sync", get(drive::setup::get).put(drive::setup::put).delete(drive::setup::delete))
        .fallback(|| async { ApiError::not_found("接口不存在") });
    let mcp = Router::new()
        .route_service("/mcp", mcp::service(state.clone()))
        .route_layer(middleware::from_fn_with_state(state.clone(), mcp::authenticate));
    Router::new().nest("/api/v1", api).merge(mcp).layer(CompressionLayer::new()).layer(TraceLayer::new_for_http()).with_state(state)
}

/// Lets the apps' pages, at `https://localhost` on Android and `capacitor://localhost` on iOS, call the
/// server: in local mode everything, otherwise the files, which are too large for the bridge to native
/// HTTP. They send the token in `Authorization`; no cookie (docs/api.md#附件). Nested, as in local
/// mode, the inner layer's headers take the outer's place.
pub(crate) fn page_cors() -> tower_http::cors::CorsLayer {
    use axum::http::{Method, header};
    tower_http::cors::CorsLayer::new()
        .allow_origin([HeaderValue::from_static("https://localhost"), HeaderValue::from_static("capacitor://localhost")])
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(std::time::Duration::from_secs(3600))
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

#[cfg(test)]
mod tests {
    use axum::http::{Request, header};
    use tower::ServiceExt;

    use super::*;

    /// In local mode in the apps the whole app has the layer, and the files routes have it again.
    #[tokio::test]
    async fn each_app_page_is_allowed_once_where_the_layers_nest() {
        let dir = std::env::temp_dir().join(format!("nexttodo-cors-{}", uuid::Uuid::now_v7()));
        let state = AppState::new(Db::open_in_memory().unwrap(), Clock::system(), Files::new(dir));
        let router = app(state).layer(page_cors());
        for origin in ["https://localhost", "capacitor://localhost"] {
            let req =
                Request::get(format!("/api/v1/files/{}", uuid::Uuid::now_v7())).header(header::ORIGIN, origin).body(Body::empty()).unwrap();
            let res = router.clone().oneshot(req).await.unwrap();
            assert_eq!(res.headers().get_all(header::ACCESS_CONTROL_ALLOW_ORIGIN).iter().collect::<Vec<_>>(), [origin]);
        }
    }
}
