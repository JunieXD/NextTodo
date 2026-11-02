//! Reading data and submitting mutations.

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use nexttodo_core::board::Snapshot;
use nexttodo_core::model::{Effect, MutationId};
use nexttodo_core::ops::Mutation;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiError, Json};
use crate::mutate::{self, Actor, Applied};
use crate::store;

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SeqSnapshot {
    seq: i64,
    #[serde(flatten)]
    snapshot: Snapshot,
}

pub async fn snapshot(State(st): State<AppState>, auth: Auth) -> Result<Json<SeqSnapshot>, ApiError> {
    st.db
        .call(move |c| Ok(SeqSnapshot { seq: store::latest_seq(c, &auth.user_id)?, snapshot: store::snapshot(c, &auth.user_id)? }))
        .await
        .map(Json)
}

#[derive(Deserialize)]
pub struct Since {
    since: i64,
}

/// One mutation in the log, as clients receive it.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct LogEntry {
    seq: i64,
    id: MutationId,
    /// The API token's name; `null` for the user's own devices.
    actor: Option<String>,
    effects: Vec<Effect>,
    created_at: i64,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct Changes {
    pub seq: i64,
    pub changes: Vec<LogEntry>,
}

pub async fn changes(
    State(st): State<AppState>,
    auth: Auth,
    query: Result<Query<Since>, QueryRejection>,
) -> Result<Json<Changes>, ApiError> {
    let Query(Since { since }) = query.map_err(|e| ApiError::invalid(e.body_text(), Some("since".into())))?;
    st.db.call(move |c| changes_since(c, &auth.user_id, since)).await.map(Json)
}

/// The changes after `since`, or a `resync` error when they are no longer kept.
pub fn changes_since(c: &Connection, owner: &str, since: i64) -> Result<Changes, ApiError> {
    let pruned: i64 = c.query_row("SELECT pruned_seq FROM users WHERE id = ?1", [owner], |r| r.get(0))?;
    if since < pruned {
        return Err(ApiError::resync());
    }
    let mut stmt =
        c.prepare_cached("SELECT seq, id, actor, effects, created_at FROM mutations WHERE owner_id = ?1 AND seq > ?2 ORDER BY seq")?;
    let rows = stmt.query_map(params![owner, since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?, r.get(4)?)))?;
    let mut changes = Vec::new();
    for row in rows {
        let (seq, id, actor, effects, created_at) = row?;
        changes.push(LogEntry { seq, id, actor, effects: serde_json::from_str(&effects)?, created_at });
    }
    Ok(Changes { seq: store::latest_seq(c, owner)?, changes })
}

pub async fn mutate(State(st): State<AppState>, auth: Auth, Json(m): Json<Mutation>) -> Result<Json<Applied>, ApiError> {
    auth.require_write()?;
    let now = st.clock.now();
    let hub = st.hub.clone();
    let applied = st
        .db
        .call(move |c| {
            let applied = mutate::apply(c, &Actor { owner: &auth.user_id, token_id: &auth.token_id, via: auth.via() }, &m, now)?;
            hub.changed(&auth.user_id);
            Ok(applied)
        })
        .await?;
    Ok(Json(applied))
}
