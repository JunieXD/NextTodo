//! Real-time sync over WebSocket. See `docs/api.md#同步协议`.

use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use nexttodo_core::model::MutationId;
use nexttodo_core::ops::Mutation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use ts_rs::TS;

use crate::AppState;
use crate::auth::{self, Auth};
use crate::error::{ApiError, ErrorBody, from_value};
use crate::mutate::{self, Actor};
use crate::sync::{LogEntry, changes_since};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Often enough that reverse proxies do not close an idle connection.
const PING_EVERY: Duration = Duration::from_secs(30);
const MAX_MESSAGE: usize = 2 << 20;

#[derive(Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export)]
pub enum ClientMessage {
    Hello {
        #[ts(optional)]
        token: Option<String>,
        since: i64,
    },
    /// Read in two steps, so that a malformed mutation can still be rejected by its id.
    Mutate {
        #[ts(as = "Mutation")]
        mutation: Value,
    },
}

#[derive(Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerMessage {
    Changes {
        seq: i64,
        changes: Vec<LogEntry>,
    },
    Resync,
    Ack {
        id: MutationId,
        seq: i64,
    },
    Reject {
        id: MutationId,
        #[serde(flatten)]
        error: ErrorBody,
    },
    Error(ErrorBody),
}

pub async fn sync(ws: WebSocketUpgrade, State(st): State<AppState>, headers: HeaderMap) -> Response {
    let presented = auth::presented_secret(&headers);
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |socket| async move {
        if let Err(e) = connection(st, socket, presented).await {
            tracing::debug!(error = %e, "sync connection ended");
        }
    })
}

async fn send(ws: &mut WebSocket, msg: ServerMessage) -> Result<(), axum::Error> {
    ws.send(Message::text(serde_json::to_string(&msg).expect("messages serialize"))).await
}

/// Tells the client what went wrong and closes the connection.
async fn fail(ws: &mut WebSocket, e: ApiError) -> Result<(), axum::Error> {
    send(ws, ServerMessage::Error(e.body())).await?;
    ws.send(Message::Close(None)).await
}

async fn connection(st: AppState, mut ws: WebSocket, presented: Option<String>) -> Result<(), axum::Error> {
    let hello = match tokio::time::timeout(HELLO_TIMEOUT, ws.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<ClientMessage>(&text).ok(),
        _ => return Ok(()),
    };
    let Some(ClientMessage::Hello { token, since }) = hello else {
        return fail(&mut ws, ApiError::invalid("连接后的第一条消息必须是 hello", None)).await;
    };
    let Some(secret) = token.or(presented) else {
        return fail(&mut ws, ApiError::unauthenticated("请先登录")).await;
    };
    let auth = match auth::authenticate(&st, secret.clone()).await {
        Ok(auth) => auth,
        Err(e) => return fail(&mut ws, e).await,
    };
    // Subscribed before the first read, so nothing committed in between is missed.
    let mut hub = st.hub.subscribe();
    let mut sent = since;
    if !push(&st, &mut ws, &auth, &mut sent, true).await? {
        return Ok(());
    }
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    loop {
        tokio::select! {
            msg = ws.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    if let Err(e) = handle(&st, &mut ws, &secret, &text).await? {
                        return fail(&mut ws, e).await;
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e),
            },
            note = hub.recv() => {
                let mine = match note {
                    Ok(owner) => *owner == *auth.user_id,
                    // Missed some notes: just look for anything new.
                    Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => return Ok(()),
                };
                if mine && !push(&st, &mut ws, &auth, &mut sent, false).await? {
                    return Ok(());
                }
            }
            _ = ping.tick() => ws.send(Message::Ping(Default::default())).await?,
        }
    }
}

/// Sends the changes after `sent`. Returns false once the connection is closed: after telling the
/// client to download everything again (it reconnects from its new snapshot), or on an error.
async fn push(st: &AppState, ws: &mut WebSocket, auth: &Auth, sent: &mut i64, always: bool) -> Result<bool, axum::Error> {
    let (owner, since) = (auth.user_id.clone(), *sent);
    match st.db.call(move |c| changes_since(c, &owner, since)).await {
        Ok(batch) => {
            if always || !batch.changes.is_empty() {
                send(ws, ServerMessage::Changes { seq: batch.seq, changes: batch.changes }).await?;
            }
            *sent = batch.seq;
            Ok(true)
        }
        Err(e) if e.code() == "resync" => {
            send(ws, ServerMessage::Resync).await?;
            ws.send(Message::Close(None)).await?;
            Ok(false)
        }
        Err(e) => {
            fail(ws, e).await?;
            Ok(false)
        }
    }
}

/// Handles a message after `hello`. A mutation is acknowledged or rejected; anything that is not a
/// mutation carrying an id cannot be answered and ends the connection.
async fn handle(st: &AppState, ws: &mut WebSocket, secret: &str, text: &str) -> Result<Result<(), ApiError>, axum::Error> {
    let Ok(ClientMessage::Mutate { mutation }) = serde_json::from_str::<ClientMessage>(text) else {
        return Ok(Err(ApiError::invalid("无法识别的消息", None)));
    };
    let Some(id) = mutation.get("id").and_then(|id| serde_json::from_value::<MutationId>(id.clone()).ok()) else {
        return Ok(Err(ApiError::invalid("改动缺少合法的 id", None)));
    };
    let reply = match apply(st, secret, mutation).await {
        Ok(seq) => ServerMessage::Ack { id, seq },
        Err(e) => ServerMessage::Reject { id, error: e.body() },
    };
    send(ws, reply).await?;
    Ok(Ok(()))
}

async fn apply(st: &AppState, secret: &str, mutation: Value) -> Result<i64, ApiError> {
    // Checked on every mutation, so a token revoked meanwhile stops working at once.
    let auth = auth::authenticate(st, secret.to_owned()).await?;
    auth.require_write()?;
    let m: Mutation = from_value(mutation)?;
    let now = st.clock.now();
    let owner = auth.user_id.clone();
    let applied =
        st.db.call(move |c| mutate::apply(c, &Actor { owner: &auth.user_id, token_id: &auth.token_id, via: auth.via() }, &m, now)).await?;
    st.hub.changed(&owner);
    Ok(applied.seq)
}
