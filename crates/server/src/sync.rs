//! Reading data and submitting mutations.

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use nexttodo_core::board::Snapshot;
use nexttodo_core::model::{Effect, MutationId};
use nexttodo_core::ops::Mutation;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiError, Json};
use crate::mutate::{self, Actor, Applied};
use crate::store;

#[derive(Serialize)]
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

#[derive(Serialize)]
pub struct Change {
    seq: i64,
    id: MutationId,
    actor: Option<String>,
    effects: Vec<Effect>,
    created_at: i64,
}

#[derive(Serialize)]
pub struct Changes {
    seq: i64,
    changes: Vec<Change>,
}

pub async fn changes(
    State(st): State<AppState>,
    auth: Auth,
    query: Result<Query<Since>, QueryRejection>,
) -> Result<Json<Changes>, ApiError> {
    let Query(Since { since }) = query.map_err(|e| ApiError::invalid(e.body_text(), Some("since".into())))?;
    st.db
        .call(move |c| {
            let pruned: i64 = c.query_row("SELECT pruned_seq FROM users WHERE id = ?1", [&auth.user_id], |r| r.get(0))?;
            if since < pruned {
                return Err(ApiError::resync());
            }
            let mut stmt =
                c.prepare("SELECT seq, id, actor, effects, created_at FROM mutations WHERE owner_id = ?1 AND seq > ?2 ORDER BY seq")?;
            let rows =
                stmt.query_map(params![auth.user_id, since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?, r.get(4)?)))?;
            let mut changes = Vec::new();
            for row in rows {
                let (seq, id, actor, effects, created_at) = row?;
                changes.push(Change { seq, id, actor, effects: serde_json::from_str(&effects)?, created_at });
            }
            Ok(Changes { seq: store::latest_seq(c, &auth.user_id)?, changes })
        })
        .await
        .map(Json)
}

pub async fn mutate(State(st): State<AppState>, auth: Auth, Json(m): Json<Mutation>) -> Result<Json<Applied>, ApiError> {
    auth.require_write()?;
    let now = st.clock.now();
    st.db
        .call(move |c| mutate::apply(c, &Actor { owner: &auth.user_id, token_id: &auth.token_id, via: auth.via() }, &m, now))
        .await
        .map(Json)
}
