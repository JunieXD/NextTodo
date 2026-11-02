//! NextTodo server: the HTTP API over SQLite. See `docs/api.md`.

pub mod auth;
pub mod db;
pub mod error;
pub mod mutate;
pub mod store;
pub mod sync;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::routing::{delete, get, post};
use tower_http::compression::CompressionLayer;
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
}

pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me))
        .route("/tokens", get(auth::list_tokens).post(auth::create_token))
        .route("/tokens/{id}", delete(auth::delete_token))
        .route("/invites", get(auth::list_invites).post(auth::create_invite))
        .route("/snapshot", get(sync::snapshot))
        .route("/changes", get(sync::changes))
        .route("/mutations", post(sync::mutate))
        .fallback(|| async { ApiError::not_found("接口不存在") });
    Router::new().nest("/api/v1", api).layer(CompressionLayer::new()).layer(TraceLayer::new_for_http()).with_state(state)
}
