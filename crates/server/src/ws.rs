//! Real-time sync over WebSocket. See `docs/api.md#同步协议`.

use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use nexttodo_core::model::MutationId;
use nexttodo_core::ops::Mutation;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use ts_rs::TS;

use crate::AppState;
use crate::auth::{self, Auth};
use crate::error::{ApiError, ErrorBody, from_value};
use crate::mutate::{self, Actor};
use crate::sync::{LogEntry, changes_since};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Often enough that reverse proxies do not close an idle connection.
const PING_EVERY: Duration = Duration::from_secs(30);
/// No pong or message for this long ends the connection.
const SILENT_LIMIT: Duration = Duration::from_secs(75);
const MAX_MESSAGE: usize = 2 << 20;

#[derive(Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export)]
pub enum ClientMessage {
    Hello {
        #[ts(optional)]
        token: Option<String>,
        since: i64,
        /// The account the client's local copy belongs to. A different one signed in meanwhile
        /// (another tab, say) must not receive or change that copy.
        user: String,
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
    let presented = same_origin(&headers).then(|| auth::presented_secret(&headers)).flatten();
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |socket| async move {
        if let Err(e) = connection(st, socket, presented).await {
            tracing::debug!(error = %e, "sync connection ended");
        }
    })
}

/// Browsers send the session cookie with a WebSocket opened by any page of the same site, so the
/// cookie only counts when the page is this one, which browsers always say in `Origin`. Apps sign in
/// with a token.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) =
        headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()).and_then(|o| o.split_once("://")).map(|(_, authority)| authority)
    else {
        return false;
    };
    // Behind a reverse proxy the page's host may arrive as `X-Forwarded-Host`.
    ["x-forwarded-host", "host"]
        .iter()
        .any(|name| headers.get(*name).and_then(|h| h.to_str().ok()).is_some_and(|h| h.eq_ignore_ascii_case(origin)))
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
    let Some(ClientMessage::Hello { token, since, user }) = hello else {
        return fail(&mut ws, ApiError::invalid("连接后的第一条消息必须是 hello", None)).await;
    };
    let Some(secret) = token.or(presented) else {
        return fail(&mut ws, ApiError::unauthenticated("请先登录")).await;
    };
    let auth = match auth::authenticate(&st, secret.clone()).await {
        Ok(auth) if auth.user_id == user => auth,
        Ok(_) => return fail(&mut ws, ApiError::unauthenticated("这台设备上登录的账号已经换了，请重新登录")).await,
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
    // A client that stopped answering is gone, even if the connection looks open.
    let mut heard = Instant::now();
    loop {
        tokio::select! {
            msg = ws.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    heard = Instant::now();
                    if let Err(e) = handle(&st, &mut ws, &secret, &text).await? {
                        return fail(&mut ws, e).await;
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                // Pongs to our pings and anything else show the client is still there.
                Some(Ok(_)) => heard = Instant::now(),
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
            _ = tokio::time::sleep_until(heard + SILENT_LIMIT) => return Ok(()),
            _ = ping.tick() => ws.send(Message::Ping(Default::default())).await?,
        }
    }
}

/// Sends the changes after `sent`. Returns false once the connection is closed: after telling the
/// client to download everything again (it reconnects from its new snapshot), or on an error.
async fn push(st: &AppState, ws: &mut WebSocket, auth: &Auth, sent: &mut i64, always: bool) -> Result<bool, axum::Error> {
    let (owner, token, since) = (auth.user_id.clone(), auth.token_id.clone(), *sent);
    let read = st.db.call(move |c| {
        // A token revoked or a session signed out since `hello` gets nothing more.
        let valid: bool =
            c.query_row("SELECT EXISTS (SELECT 1 FROM tokens WHERE id = ?1 AND user_id = ?2)", params![token, owner], |r| r.get(0))?;
        if !valid {
            return Err(ApiError::unauthenticated("登录已失效，请重新登录"));
        }
        changes_since(c, &owner, since)
    });
    match read.await {
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

/// Handles a message after `hello`. A mutation is acknowledged, or rejected when the change itself
/// does not hold. Anything that is not a mutation carrying an id cannot be answered and ends the
/// connection, as does a mutation after the session ended or one the server failed at: that is no
/// fault of the change, which may still go through in the next session or after reconnecting.
async fn handle(st: &AppState, ws: &mut WebSocket, secret: &str, text: &str) -> Result<Result<(), ApiError>, axum::Error> {
    let Ok(ClientMessage::Mutate { mutation }) = serde_json::from_str::<ClientMessage>(text) else {
        return Ok(Err(ApiError::invalid("无法识别的消息", None)));
    };
    let Some(id) = mutation.get("id").and_then(|id| serde_json::from_value::<MutationId>(id.clone()).ok()) else {
        return Ok(Err(ApiError::invalid("改动缺少合法的 id", None)));
    };
    let reply = match apply(st, secret, mutation).await {
        Ok(seq) => ServerMessage::Ack { id, seq },
        Err(e) if matches!(e.code(), "unauthenticated" | "internal") => return Ok(Err(e)),
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
    let hub = st.hub.clone();
    st.db
        .call(move |c| {
            let applied = mutate::apply(c, &Actor { owner: &auth.user_id, token_id: &auth.token_id, via: auth.via() }, &m, now)?;
            hub.changed(&auth.user_id);
            Ok(applied.seq)
        })
        .await
}
