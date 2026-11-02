//! Accounts, sessions, API tokens and invites.

use std::sync::LazyLock;

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use nexttodo_core::model::line;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiError, Json};

const COOKIE: &str = "nt_session";
const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// Skip writing `last_used_at` when it was updated less than this long ago.
const LAST_USED_RESOLUTION_MS: i64 = 60_000;

fn random_string(len: usize) -> String {
    (0..len).map(|_| char::from(BASE62[rand::random_range(0..BASE62.len())])).collect()
}

/// 43 base62 characters carry 256 bits.
fn new_secret() -> String {
    format!("nt_{}", random_string(43))
}

fn secret_hash(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}

fn hash_password(password: &str) -> String {
    Argon2::default().hash_password(password.as_bytes()).expect("argon2 hashing").to_string()
}

fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// Checked when the username does not exist, so that a failed login takes as long either way.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| hash_password(&random_string(16)));

/// Each password hash takes about 19 MiB and tens of milliseconds. Anyone can ask for one by trying
/// to log in, so only this many run at once and the rest wait their turn.
static HASHING: Semaphore = Semaphore::const_new(2);

/// Runs password hashing or verification on a blocking thread, a few at a time.
async fn hashing<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let _permit = HASHING.acquire().await.expect("the semaphore is never closed");
    tokio::task::spawn_blocking(f).await.expect("hashing task panicked")
}

macro_rules! field {
    ($name:ident, $check:expr) => {
        #[derive(Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(s: String) -> Result<Self, String> {
                let check: fn(String) -> Result<String, String> = $check;
                check(s).map(Self)
            }
        }
    };
}

field!(Username, |s| if (3..=32).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
    Ok(s)
} else {
    Err("用户名要 3–32 位，只能用字母、数字和下划线".into())
});
field!(Password, |s| if (8..=128).contains(&s.chars().count()) { Ok(s) } else { Err("密码要 8–128 位".into()) });
field!(DisplayName, |s| line(s, "显示名", 50));
field!(DeviceName, |s| line(s, "设备名", 100));
field!(TokenName, |s| line(s, "令牌名", 50));

macro_rules! sql_enum {
    ($name:ident { $($variant:ident = $text:literal),* }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum $name { $($variant),* }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(match self { $(Self::$variant => $text),* }.into())
            }
        }

        impl FromSql for $name {
            fn column_result(v: ValueRef<'_>) -> FromSqlResult<Self> {
                match v.as_str()? { $($text => Ok(Self::$variant),)* _ => Err(FromSqlError::InvalidType) }
            }
        }
    };
}

sql_enum!(Scope { Read = "read", Write = "write" });
sql_enum!(TokenKind { Session = "session", Api = "api" });

/// The authenticated caller.
pub struct Auth {
    pub user_id: String,
    pub token_id: String,
    pub kind: TokenKind,
    pub scope: Scope,
    pub name: String,
    pub is_admin: bool,
}

impl Auth {
    /// The source recorded on changes: the token name for API tokens, none for the user's own devices.
    pub fn via(&self) -> Option<&str> {
        (self.kind == TokenKind::Api).then_some(self.name.as_str())
    }

    pub fn require_write(&self) -> Result<(), ApiError> {
        if self.scope == Scope::Write { Ok(()) } else { Err(ApiError::forbidden("这个令牌是只读的")) }
    }

    fn require_session(&self) -> Result<(), ApiError> {
        if self.kind == TokenKind::Session {
            Ok(())
        } else {
            Err(ApiError::forbidden("令牌和邀请码只能在登录的设备上管理"))
        }
    }
}

fn presented_secret(headers: &HeaderMap) -> Option<String> {
    let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if let Some(secret) = bearer {
        return Some(secret.trim().to_owned());
    }
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| pair.trim().strip_prefix("nt_session="))
        .map(str::to_owned)
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let secret = presented_secret(&parts.headers).ok_or_else(|| ApiError::unauthenticated("请先登录"))?;
        let now = state.clock.now();
        state
            .db
            .call(move |c| {
                let (auth, last_used): (Auth, Option<i64>) = c
                    .query_row(
                        "SELECT t.id, t.user_id, t.kind, t.scope, t.name, u.is_admin, t.last_used_at
                         FROM tokens t JOIN users u ON u.id = t.user_id WHERE t.secret_hash = ?1",
                        [secret_hash(&secret)],
                        |r| {
                            let auth = Auth {
                                token_id: r.get(0)?,
                                user_id: r.get(1)?,
                                kind: r.get(2)?,
                                scope: r.get(3)?,
                                name: r.get(4)?,
                                is_admin: r.get(5)?,
                            };
                            Ok((auth, r.get(6)?))
                        },
                    )
                    .optional()?
                    .ok_or_else(|| ApiError::unauthenticated("登录已失效，请重新登录"))?;
                if last_used.is_none_or(|t| now - t >= LAST_USED_RESOLUTION_MS) {
                    c.execute("UPDATE tokens SET last_used_at = ?1 WHERE id = ?2", params![now, auth.token_id])?;
                }
                Ok(auth)
            })
            .await
    }
}

#[derive(Serialize)]
pub struct User {
    id: String,
    username: String,
    display_name: String,
    is_admin: bool,
}

fn load_user(c: &Connection, id: &str) -> rusqlite::Result<User> {
    c.query_row("SELECT id, username, display_name, is_admin FROM users WHERE id = ?1", [id], |r| {
        Ok(User { id: r.get(0)?, username: r.get(1)?, display_name: r.get(2)?, is_admin: r.get(3)? })
    })
}

fn create_user(
    c: &Connection,
    username: &str,
    display_name: &str,
    password_hash: &str,
    is_admin: bool,
    now: i64,
) -> Result<String, ApiError> {
    let taken: bool = c.query_row("SELECT EXISTS (SELECT 1 FROM users WHERE username = ?1)", [username], |r| r.get(0))?;
    if taken {
        return Err(ApiError::conflict("用户名已被占用"));
    }
    let id = Uuid::now_v7().to_string();
    c.execute(
        "INSERT INTO users (id, username, display_name, password_hash, is_admin, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, username, display_name, password_hash, is_admin, now],
    )?;
    Ok(id)
}

/// Creates an administrator; used by the `create-admin` command.
pub async fn create_admin(state: &AppState, username: String, display_name: String, password: String) -> Result<(), ApiError> {
    let username = Username::try_from(username).map_err(|m| ApiError::invalid(m, Some("username".into())))?;
    let display_name = DisplayName::try_from(display_name).map_err(|m| ApiError::invalid(m, Some("display_name".into())))?;
    let password = Password::try_from(password).map_err(|m| ApiError::invalid(m, Some("password".into())))?;
    let hash = hashing(move || hash_password(&password.0)).await;
    let now = state.clock.now();
    state.db.call(move |c| create_user(c, &username.0, &display_name.0, &hash, true, now).map(drop)).await
}

#[derive(Serialize)]
struct Session {
    secret: String,
    user: User,
}

fn create_session(c: &Connection, user_id: &str, device: &str, now: i64) -> rusqlite::Result<String> {
    let secret = new_secret();
    c.execute(
        "INSERT INTO tokens (id, user_id, kind, name, scope, secret_hash, created_at, last_used_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![Uuid::now_v7().to_string(), user_id, TokenKind::Session, device, Scope::Write, secret_hash(&secret), now],
    )?;
    Ok(secret)
}

fn with_session_cookie(session: Session) -> Response {
    let cookie = format!("{COOKIE}={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=34560000", session.secret);
    ([(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("cookie is ASCII"))], Json(session)).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    username: String,
    password: String,
    device: DeviceName,
}

pub async fn login(State(st): State<AppState>, Json(req): Json<Login>) -> Result<Response, ApiError> {
    let username = req.username;
    let found: Option<(String, String)> = st
        .db
        .call(move |c| {
            Ok(c.query_row("SELECT id, password_hash FROM users WHERE username = ?1", [username], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?)
        })
        .await?;
    let (password, hash) = (req.password, found.as_ref().map_or_else(|| DUMMY_HASH.clone(), |(_, h)| h.clone()));
    let matches = hashing(move || verify_password(&password, &hash)).await;
    let Some((user_id, _)) = found.filter(|_| matches) else {
        return Err(ApiError::unauthenticated("用户名或密码不对"));
    };
    let (now, device) = (st.clock.now(), req.device.0);
    let session = st
        .db
        .call(move |c| {
            let secret = create_session(c, &user_id, &device, now)?;
            Ok(Session { secret, user: load_user(c, &user_id)? })
        })
        .await?;
    Ok(with_session_cookie(session))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Register {
    invite: String,
    username: Username,
    password: Password,
    display_name: DisplayName,
    device: DeviceName,
}

fn check_invite(c: &Connection, code: &str) -> Result<(), ApiError> {
    let used_by: Option<Option<String>> = c.query_row("SELECT used_by FROM invites WHERE code = ?1", [code], |r| r.get(0)).optional()?;
    match used_by {
        None => Err(ApiError::not_found("邀请码不存在")),
        Some(Some(_)) => Err(ApiError::conflict("邀请码已经用过了")),
        Some(None) => Ok(()),
    }
}

pub async fn register(State(st): State<AppState>, Json(req): Json<Register>) -> Result<Response, ApiError> {
    let Register { invite, username, password, display_name, device } = req;
    // Checked before the costly hashing, and again in the transaction that uses it up.
    let code = invite.clone();
    st.db.call(move |c| check_invite(c, &code)).await?;
    let hash = hashing(move || hash_password(&password.0)).await;
    let now = st.clock.now();
    let session = st
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            check_invite(&tx, &invite)?;
            let user_id = create_user(&tx, &username.0, &display_name.0, &hash, false, now)?;
            tx.execute("UPDATE invites SET used_by = ?1, used_at = ?2 WHERE code = ?3", params![user_id, now, invite])?;
            let secret = create_session(&tx, &user_id, &device.0, now)?;
            let user = load_user(&tx, &user_id)?;
            tx.commit()?;
            Ok(Session { secret, user })
        })
        .await?;
    Ok(with_session_cookie(session))
}

pub async fn logout(State(st): State<AppState>, auth: Auth) -> Result<Response, ApiError> {
    auth.require_session()?;
    st.db.call(move |c| Ok(c.execute("DELETE FROM tokens WHERE id = ?1", [auth.token_id])?)).await?;
    let cookie = format!("{COOKIE}=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0");
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("cookie is ASCII"))]).into_response())
}

pub async fn me(State(st): State<AppState>, auth: Auth) -> Result<Json<User>, ApiError> {
    st.db.call(move |c| Ok(load_user(c, &auth.user_id)?)).await.map(Json)
}

#[derive(Serialize)]
pub struct TokenInfo {
    id: String,
    kind: TokenKind,
    name: String,
    scope: Scope,
    created_at: i64,
    last_used_at: Option<i64>,
    current: bool,
}

pub async fn list_tokens(State(st): State<AppState>, auth: Auth) -> Result<Json<Vec<TokenInfo>>, ApiError> {
    auth.require_session()?;
    st.db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, kind, name, scope, created_at, last_used_at FROM tokens WHERE user_id = ?1 ORDER BY created_at DESC",
            )?;
            let tokens = stmt.query_map([&auth.user_id], |r| {
                let id: String = r.get(0)?;
                Ok(TokenInfo {
                    current: id == auth.token_id,
                    id,
                    kind: r.get(1)?,
                    name: r.get(2)?,
                    scope: r.get(3)?,
                    created_at: r.get(4)?,
                    last_used_at: r.get(5)?,
                })
            })?;
            Ok(tokens.collect::<rusqlite::Result<_>>()?)
        })
        .await
        .map(Json)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewToken {
    name: TokenName,
    scope: Scope,
}

#[derive(Serialize)]
pub struct CreatedToken {
    token: TokenInfo,
    secret: String,
}

pub async fn create_token(State(st): State<AppState>, auth: Auth, Json(req): Json<NewToken>) -> Result<Json<CreatedToken>, ApiError> {
    auth.require_session()?;
    let now = st.clock.now();
    st.db
        .call(move |c| {
            let (id, secret) = (Uuid::now_v7().to_string(), new_secret());
            c.execute(
                "INSERT INTO tokens (id, user_id, kind, name, scope, secret_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, auth.user_id, TokenKind::Api, req.name.0, req.scope, secret_hash(&secret), now],
            )?;
            let token = TokenInfo {
                id,
                kind: TokenKind::Api,
                name: req.name.0,
                scope: req.scope,
                created_at: now,
                last_used_at: None,
                current: false,
            };
            Ok(CreatedToken { token, secret })
        })
        .await
        .map(Json)
}

pub async fn delete_token(State(st): State<AppState>, auth: Auth, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    auth.require_session()?;
    st.db
        .call(move |c| match c.execute("DELETE FROM tokens WHERE id = ?1 AND user_id = ?2", params![id, auth.user_id])? {
            0 => Err(ApiError::not_found("令牌不存在")),
            _ => Ok(StatusCode::NO_CONTENT),
        })
        .await
}

#[derive(Serialize)]
pub struct Invite {
    code: String,
    created_at: i64,
    /// Username of whoever registered with it.
    used_by: Option<String>,
    used_at: Option<i64>,
}

fn require_admin(auth: &Auth) -> Result<(), ApiError> {
    auth.require_session()?;
    if auth.is_admin { Ok(()) } else { Err(ApiError::forbidden("只有管理员能管理邀请码")) }
}

pub async fn list_invites(State(st): State<AppState>, auth: Auth) -> Result<Json<Vec<Invite>>, ApiError> {
    require_admin(&auth)?;
    st.db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT i.code, i.created_at, u.username, i.used_at FROM invites i LEFT JOIN users u ON u.id = i.used_by
                 WHERE i.created_by = ?1 ORDER BY i.created_at DESC",
            )?;
            let invites = stmt.query_map([&auth.user_id], |r| {
                Ok(Invite { code: r.get(0)?, created_at: r.get(1)?, used_by: r.get(2)?, used_at: r.get(3)? })
            })?;
            Ok(invites.collect::<rusqlite::Result<_>>()?)
        })
        .await
        .map(Json)
}

pub async fn create_invite(State(st): State<AppState>, auth: Auth) -> Result<Json<Invite>, ApiError> {
    require_admin(&auth)?;
    let now = st.clock.now();
    st.db
        .call(move |c| {
            let code = random_string(12);
            c.execute("INSERT INTO invites (code, created_by, created_at) VALUES (?1, ?2, ?3)", params![code, auth.user_id, now])?;
            Ok(Invite { code, created_at: now, used_by: None, used_at: None })
        })
        .await
        .map(Json)
}
