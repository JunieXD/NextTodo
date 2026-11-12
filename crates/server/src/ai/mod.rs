//! AI organizing: jobs turned into tasks in the background, or into suggestions for a folder's
//! tasks, with each user's own model. See `docs/ai.md`.

pub mod llm;
pub mod organize;
pub mod secret;
pub mod tidy;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use nexttodo_core::board::Board;
use nexttodo_core::model::{AiJobId, Date, JobKind, JobState, MutationId, Suggestion};
use nexttodo_core::ops::Op;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{Notify, Semaphore};
use ts_rs::TS;
use uuid::Uuid;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiError, Json};
use crate::mutate::{self, Actor};
use crate::tools::args::PlanTask;
use crate::{store, tools};
use llm::{Api, Config, PROVIDERS, Provider, Thinking};
use secret::SecretKey;

/// Who the changes made for jobs are by.
pub const ACTOR: &str = "AI 整理";

/// How long a job's model may take to answer.
const TIMEOUT: Duration = Duration::from_secs(300);
const TEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How many jobs are organized at once, of all users together.
const SLOTS: usize = 4;

fn actor(owner: &str) -> Actor<'_> {
    Actor { owner, token_id: "", via: Some(ACTOR) }
}

pub struct Ai {
    /// Opens the API keys kept on the server; without it they can only be kept on devices.
    secret: Option<SecretKey>,
    api: Arc<dyn Api>,
    /// Users with a job being organized: one at a time each, as some providers take one request at
    /// a time.
    busy: Mutex<HashSet<String>>,
    slots: Semaphore,
    /// A job has ended, and its user may have another waiting.
    freed: Notify,
}

impl Ai {
    pub fn new(secret: Option<SecretKey>, api: Arc<dyn Api>) -> Self {
        Ai { secret, api, busy: Mutex::default(), slots: Semaphore::new(SLOTS), freed: Notify::new() }
    }

    /// `owner`'s turn to have a job organized; none while they have one being organized.
    fn claim(self: &Arc<Self>, owner: &str) -> Option<Turn> {
        let mut busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        busy.insert(owner.to_owned()).then(|| Turn { ai: self.clone(), owner: owner.to_owned() })
    }
}

/// A user's turn to have a job organized, over when dropped.
struct Turn {
    ai: Arc<Ai>,
    owner: String,
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.ai.busy.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.owner);
        self.ai.freed.notify_one();
    }
}

impl ToSql for Thinking {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Thinking {
    fn column_result(v: ValueRef<'_>) -> FromSqlResult<Self> {
        Thinking::parse(v.as_str()?).ok_or(FromSqlError::InvalidType)
    }
}

/// Settings kept on the server, the key still sealed.
struct Kept {
    provider: String,
    base_url: String,
    model: String,
    thinking: Thinking,
    api_key: Vec<u8>,
}

fn kept(c: &Connection, owner: &str) -> rusqlite::Result<Option<Kept>> {
    c.query_row("SELECT provider, base_url, model, thinking, api_key FROM ai_settings WHERE user_id = ?1", [owner], |r| {
        Ok(Kept { provider: r.get(0)?, base_url: r.get(1)?, model: r.get(2)?, thinking: r.get(3)?, api_key: r.get(4)? })
    })
    .optional()
}

impl Kept {
    /// The key; none when this server's key does not open it, having changed say.
    fn key(&self, secret: Option<&SecretKey>, owner: &str) -> Option<String> {
        secret?.open(owner, &self.api_key)
    }

    fn config(self, api_key: String) -> Config {
        Config { provider: self.provider, base_url: self.base_url, model: self.model, thinking: self.thinking, api_key }
    }
}

const UNOPENED: &str = "服务器解不开保存的 API 密钥，请在设置里重新填写";

// The queue.

/// Organizes the jobs of users whose settings are on the server, until the server stops: each change
/// to a user's data, and each job that ends, has it look for a job to start.
pub async fn run_queue(st: AppState) {
    let mut changes = st.hub.subscribe();
    let mut owner = None;
    loop {
        let _ = start_waiting(&st, owner.take()).await;
        owner = tokio::select! {
            note = changes.recv() => match note {
                Ok(o) => Some(o.to_string()),
                Err(RecvError::Lagged(_)) => None,
                Err(RecvError::Closed) => return,
            },
            () = st.ai.freed.notified() => None,
        };
    }
}

/// Fails the jobs that were being organized when the server stopped.
pub async fn interrupted(st: &AppState) -> Result<(), ApiError> {
    let (now, hub) = (st.clock.now(), st.hub.clone());
    st.db
        .call(move |c| {
            let running: Vec<(String, AiJobId)> = c
                .prepare("SELECT owner_id, id FROM ai_jobs WHERE state = 'running'")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            for (owner, id) in running {
                mutate::set_job_state(
                    c,
                    &actor(&owner),
                    id,
                    JobState::Running,
                    JobState::Failed,
                    Some("服务器重启，整理中断".into()),
                    now,
                )?;
                hub.changed(&owner);
            }
            Ok(())
        })
        .await
}

/// Starts the oldest waiting job of each user with settings on the server, `owner` alone if given,
/// unless they have one being organized.
async fn start_waiting(st: &AppState, owner: Option<String>) -> Result<(), ApiError> {
    let waiting = st
        .db
        .call(move |c| {
            let rows = c
                .prepare_cached(
                    "SELECT j.owner_id, j.id FROM ai_jobs j JOIN ai_settings s ON s.user_id = j.owner_id
                     WHERE j.state = 'waiting' AND (?1 IS NULL OR j.owner_id = ?1) ORDER BY j.id",
                )?
                .query_map([owner], |r| Ok((r.get::<_, String>(0)?, r.get::<_, AiJobId>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let mut seen = HashSet::new();
    for (owner, job) in waiting {
        if !seen.insert(owner.clone()) {
            continue;
        }
        let Some(turn) = st.ai.claim(&owner) else { continue };
        let ai = st.ai.clone();
        // Gone meanwhile, to be kept on the user's devices: the job waits for one of them.
        let Some(kept) = st.db.call(move |c| Ok(kept(c, &owner)?)).await? else { continue };
        match kept.key(ai.secret.as_ref(), &turn.owner) {
            Some(key) => {
                start(st, turn, job, kept.config(key)).await?;
            }
            None => {
                let (owner, now, hub) = (turn.owner.clone(), st.clock.now(), st.hub.clone());
                st.db
                    .call(move |c| {
                        mutate::set_job_state(c, &actor(&owner), job, JobState::Waiting, JobState::Failed, Some(UNOPENED.into()), now)?;
                        hub.changed(&owner);
                        Ok(())
                    })
                    .await?;
            }
        }
    }
    Ok(())
}

/// Starts organizing `job` in `turn`; whether it was still waiting to be.
async fn start(st: &AppState, turn: Turn, job: AiJobId, config: Config) -> Result<bool, ApiError> {
    let (owner, now, hub) = (turn.owner.clone(), st.clock.now(), st.hub.clone());
    let started = st
        .db
        .call(move |c| {
            let started = mutate::set_job_state(c, &actor(&owner), job, JobState::Waiting, JobState::Running, None, now)?.is_some();
            if started {
                hub.changed(&owner);
            }
            Ok(started)
        })
        .await?;
    if started {
        tokio::spawn(organize(st.clone(), turn, job, config));
    }
    Ok(started)
}

/// How a job ended.
#[derive(Debug)]
enum Ended {
    Done,
    Failed,
    /// It was deleted, or is no longer being organized for some other reason: what was organized
    /// for it is thrown away.
    Dropped,
}

/// What a job's model was shown, to read its answer by.
enum Shown {
    Organize(HashSet<i64>),
    Tidy(tidy::Shown),
}

/// What a job's model answered.
enum Answer {
    Tasks(Vec<PlanTask>),
    Suggestions(Vec<Suggestion>),
}

/// Asks the model and adds what it answers, or keeps its suggestions, or fails the job.
async fn organize(st: AppState, turn: Turn, job: AiJobId, config: Config) {
    let _slot = st.ai.slots.acquire().await.expect("the semaphore stays open");
    let began = Instant::now();
    let owner = turn.owner.clone();
    let read = st
        .db
        .call(move |c| {
            let Some(j) = store::load_job(c, &owner, job)?.filter(|j| j.state == JobState::Running) else { return Ok(None) };
            let snap = store::snapshot(c, &owner)?;
            Ok(Some(match j.kind {
                JobKind::Organize => {
                    // Organizing reads no task's status, for which the board needs the user's day.
                    let (prompt, shown) = organize::prompt(&j, &Board::new(&snap, Date::utc(j.created_at)));
                    (prompt, Shown::Organize(shown))
                }
                JobKind::Tidy => {
                    let (prompt, shown) = tidy::prompt(&j, snap);
                    (prompt, Shown::Tidy(shown))
                }
            }))
        })
        .await;
    let (mut status, outcome) = match read {
        Ok(None) => return,
        Ok(Some((prompt, shown))) => {
            let api = st.ai.api.clone();
            let answer = tokio::task::spawn_blocking(move || api.chat(&config, &prompt, true, TIMEOUT)).await.expect("chat task panicked");
            match answer {
                Ok(text) => (
                    None,
                    match shown {
                        Shown::Organize(shown) => organize::parse(&text, &shown).map(Answer::Tasks),
                        Shown::Tidy(shown) => tidy::parse(&text, &shown).map(Answer::Suggestions),
                    },
                ),
                Err(f) => (f.status, Err(f.message)),
            }
        }
        Err(e) => (None, Err(e.describe())),
    };
    if outcome.is_ok() {
        status = Some(200);
    }
    let (owner, now, hub) = (turn.owner.clone(), st.clock.now(), st.hub.clone());
    let ended = st
        .db
        .call(move |c| {
            let ended = finish(c, &owner, job, outcome, now)?;
            hub.changed(&owner);
            Ok(ended)
        })
        .await;
    tracing::info!(job = %job, ended = ?ended.ok(), status, ms = began.elapsed().as_millis(), "ai job");
}

/// Ends a job being organized: its tasks added and the job deleted, in one change; a tidying ready,
/// with its suggestions; or the job failed.
fn finish(c: &mut Connection, owner: &str, id: AiJobId, outcome: Result<Answer, String>, now: i64) -> Result<Ended, ApiError> {
    let actor = actor(owner);
    let failure = match outcome {
        Err(message) => message,
        Ok(Answer::Suggestions(suggestions)) => {
            let ready = mutate::suggest(c, &actor, id, suggestions, now)?;
            return Ok(if ready.is_some() { Ended::Done } else { Ended::Dropped });
        }
        Ok(Answer::Tasks(tasks)) => {
            let tx = c.transaction()?;
            let Some(job) = store::load_job(&tx, owner, id)?.filter(|j| j.state == JobState::Running) else { return Ok(Ended::Dropped) };
            let snap = store::snapshot(&tx, owner)?;
            let applied = tools::plan_into(&Board::new(&snap, Date::utc(now)), job.folder_id, tasks).and_then(|mut ops| {
                ops.push(Op::AiJobDelete { id });
                let effects = mutate::run(&tx, &actor, &ops, now)?;
                Ok((ops, effects))
            });
            match applied {
                Ok((ops, effects)) => {
                    mutate::record(&tx, &actor, MutationId(Uuid::now_v7()), &ops, &effects, now)?;
                    tx.commit()?;
                    return Ok(Ended::Done);
                }
                Err(e) if e.code() == "internal" => return Err(e),
                // What the plan did so far goes as the transaction is dropped.
                Err(e) => e.describe(),
            }
        }
    };
    let failed = mutate::set_job_state(c, &actor, id, JobState::Running, JobState::Failed, Some(failure), now)?;
    Ok(if failed.is_some() { Ended::Failed } else { Ended::Dropped })
}

// The API.

#[derive(Serialize, TS)]
#[ts(export, rename = "AiProviders")]
pub struct Providers {
    providers: &'static [Provider],
    /// Whether settings can be kept on the server.
    sync: bool,
}

pub async fn providers(State(st): State<AppState>, _auth: Auth) -> Json<Providers> {
    Json(Providers { providers: PROVIDERS, sync: st.ai.secret.is_some() })
}

/// Settings to keep on the server or to try out; without a key, with the one kept.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "AiSettingsInput")]
pub struct SettingsInput {
    provider: String,
    base_url: String,
    model: String,
    thinking: Thinking,
    #[ts(optional)]
    api_key: Option<String>,
}

/// Settings kept on the server, as shown: the key by its last characters.
#[derive(Serialize, TS)]
#[ts(export, rename = "AiSettings")]
pub struct SettingsView {
    provider: String,
    base_url: String,
    model: String,
    thinking: Thinking,
    /// The key's last 4 characters; `null` when the server cannot open it, and it has to be given again.
    key_end: Option<String>,
}

fn view(k: &Kept, key: Option<&str>) -> SettingsView {
    SettingsView {
        provider: k.provider.clone(),
        base_url: k.base_url.clone(),
        model: k.model.clone(),
        thinking: k.thinking,
        key_end: key.map(|k| k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect()),
    }
}

fn checked(c: Config) -> Result<Config, ApiError> {
    c.check().map_err(|m| ApiError::invalid(m, None))?;
    Ok(c)
}

/// The config `input` gives, with the key kept on the server when it gives none.
async fn resolve(st: &AppState, owner: String, input: SettingsInput) -> Result<Config, ApiError> {
    let api_key = match input.api_key {
        Some(key) => key,
        None => {
            let ai = st.ai.clone();
            st.db
                .call(move |c| {
                    let k = kept(c, &owner)?.ok_or_else(|| ApiError::invalid("请填写 API 密钥", Some("api_key".into())))?;
                    k.key(ai.secret.as_ref(), &owner).ok_or_else(|| ApiError::conflict(UNOPENED))
                })
                .await?
        }
    };
    checked(Config { provider: input.provider, base_url: input.base_url, model: input.model, thinking: input.thinking, api_key })
}

pub async fn get_settings(State(st): State<AppState>, auth: Auth) -> Result<Json<Option<SettingsView>>, ApiError> {
    auth.require_device()?;
    let ai = st.ai.clone();
    st.db
        .call(move |c| {
            let owner = auth.user_id;
            Ok(kept(c, &owner)?.map(|k| view(&k, k.key(ai.secret.as_ref(), &owner).as_deref())))
        })
        .await
        .map(Json)
}

pub async fn put_settings(
    State(st): State<AppState>,
    auth: Auth,
    Json(input): Json<SettingsInput>,
) -> Result<Json<SettingsView>, ApiError> {
    auth.require_device()?;
    if st.ai.secret.is_none() {
        return Err(ApiError::conflict("服务器没有设置 NEXTTODO_SECRET_KEY，AI 整理的配置只能保存在本机"));
    }
    let config = resolve(&st, auth.user_id.clone(), input).await?;
    let (ai, hub, now) = (st.ai.clone(), st.hub.clone(), st.clock.now());
    st.db
        .call(move |c| {
            let owner = auth.user_id;
            let sealed = ai.secret.as_ref().expect("checked above").seal(&owner, &config.api_key);
            c.execute(
                "INSERT INTO ai_settings (user_id, provider, base_url, model, thinking, api_key, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (user_id) DO UPDATE SET provider = ?2, base_url = ?3, model = ?4, thinking = ?5, api_key = ?6, updated_at = ?7",
                params![owner, config.provider, config.base_url, config.model, config.thinking, sealed, now],
            )?;
            // The queue hears of it too, and starts the jobs that were waiting for settings.
            hub.changed(&owner);
            let k = Kept { provider: config.provider, base_url: config.base_url, model: config.model, thinking: config.thinking, api_key: sealed };
            Ok(view(&k, Some(&config.api_key)))
        })
        .await
        .map(Json)
}

pub async fn delete_settings(State(st): State<AppState>, auth: Auth) -> Result<StatusCode, ApiError> {
    auth.require_device()?;
    st.db
        .call(move |c| {
            c.execute("DELETE FROM ai_settings WHERE user_id = ?1", [auth.user_id])?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Serialize, TS)]
#[ts(export, rename = "AiTestResult")]
pub struct TestResult {
    ok: bool,
    message: String,
}

/// Sends a minimal chat with the settings given, as a job would.
pub async fn test(State(st): State<AppState>, auth: Auth, Json(input): Json<SettingsInput>) -> Result<Json<TestResult>, ApiError> {
    auth.require_device()?;
    let config = resolve(&st, auth.user_id, input).await?;
    let api = st.ai.api.clone();
    let began = Instant::now();
    let answer = tokio::task::spawn_blocking(move || api.chat(&config, &organize::test_prompt(), true, TEST_TIMEOUT))
        .await
        .expect("chat task panicked");
    let secs = began.elapsed().as_secs_f32();
    Ok(Json(match answer {
        Ok(text) if serde_json::from_str::<serde_json::Value>(organize::unwrap(&text)).is_ok() => {
            TestResult { ok: true, message: format!("连接成功，用时 {secs:.1} 秒") }
        }
        Ok(_) => TestResult { ok: false, message: "连接成功，但模型没有按要求只输出 JSON，整理时可能失败".into() },
        Err(f) => TestResult { ok: false, message: f.message },
    }))
}

#[derive(Serialize, TS)]
#[ts(export, rename = "AiModels")]
pub struct Models {
    models: Vec<String>,
}

pub async fn models(State(st): State<AppState>, auth: Auth, Json(input): Json<SettingsInput>) -> Result<Json<Models>, ApiError> {
    auth.require_device()?;
    let config = resolve(&st, auth.user_id, input).await?;
    let api = st.ai.api.clone();
    let listed = tokio::task::spawn_blocking(move || api.models(&config)).await.expect("models task panicked");
    listed.map(|models| Json(Models { models })).map_err(|f| ApiError::upstream(f.message))
}

/// A device keeping its settings to itself hands them over for one job, which starts at once. They
/// stay in memory only for as long as it takes.
pub async fn run(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Json(config): Json<Config>,
) -> Result<StatusCode, ApiError> {
    auth.require_device()?;
    let id = Uuid::parse_str(&id).ok().and_then(|u| AiJobId::try_from(u).ok()).ok_or_else(|| ApiError::not_found("AI 整理请求不存在"))?;
    let config = checked(config)?;
    let turn = st.ai.claim(&auth.user_id).ok_or_else(|| ApiError::conflict("正在整理另一条请求，请稍后重试"))?;
    if start(&st, turn, id, config).await? { Ok(StatusCode::ACCEPTED) } else { Err(ApiError::conflict("这条请求不在等待整理")) }
}
