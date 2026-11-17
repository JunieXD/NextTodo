//! AI organizing: jobs turned into tasks in the background, or into suggestions for a folder's
//! tasks, with each user's own model. See `docs/ai.md`.

pub mod llm;
pub mod organize;
pub mod secret;
pub mod tidy;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use nexttodo_core::board::{Board, Now};
use nexttodo_core::model::{AiJob, AiJobId, Attachment, FolderId, Holder, JobKind, JobState, MutationId, NoteText, Suggestion, TaskId};
use nexttodo_core::ops::Op;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{Notify, Semaphore, oneshot};
use ts_rs::TS;
use uuid::Uuid;

use crate::auth::Auth;
use crate::drive::{self, Drive, Maker, Route, StepFn};
use crate::error::{ApiError, Json};
use crate::mutate::{self, Actor, Done};
use crate::tools::args::PlanTask;
use crate::{AppState, Hub};
use crate::{store, tools};
use llm::{Api, Config, Endpoint, Failure, PROVIDERS, Provider, Thinking};
use secret::SecretKey;

/// Who the changes made for jobs are by.
pub const ACTOR: &str = "AI 整理";

/// How long a job's model may take to answer.
const TIMEOUT: Duration = Duration::from_secs(300);
const TEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How many jobs are organized at once, of all users together.
const SLOTS: usize = 4;

fn actor(owner: &str) -> Actor<'_> {
    Actor { user: owner, token_id: "", via: Some(ACTOR) }
}

fn maker(owner: &str) -> Maker {
    Maker { user: owner.to_owned(), token_id: String::new(), via: Some(ACTOR.to_owned()) }
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
    vision: bool,
    api_key: Vec<u8>,
}

fn kept(c: &Connection, owner: &str) -> rusqlite::Result<Option<Kept>> {
    c.query_row("SELECT provider, base_url, model, thinking, vision, api_key FROM ai_settings WHERE user_id = ?1", [owner], |r| {
        Ok(Kept { provider: r.get(0)?, base_url: r.get(1)?, model: r.get(2)?, thinking: r.get(3)?, vision: r.get(4)?, api_key: r.get(5)? })
    })
    .optional()
}

impl Kept {
    /// The key; none when this server's key does not open it, having changed say.
    fn key(&self, secret: Option<&SecretKey>, owner: &str) -> Option<String> {
        secret?.open(owner, &self.api_key)
    }

    fn config(self, api_key: String) -> Config {
        Config {
            provider: self.provider,
            base_url: self.base_url,
            model: self.model,
            thinking: self.thinking,
            vision: self.vision,
            api_key,
        }
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

/// Fails the jobs that were being organized when the server stopped. Syncing through a drive, those of
/// the rounds this device started (docs/local.md#运行): queued before any request, and not waited for,
/// as the drive may be out of reach.
pub async fn interrupted(st: &AppState) -> Result<(), ApiError> {
    let _direct = match st.drive.route().await {
        Route::Drive(drive) => {
            cut_rounds(st, &drive, own_rounds(st).await?, CUT);
            return Ok(());
        }
        Route::Direct(held) => held,
    };
    let (now, hub) = (st.clock.now(), st.hub.clone());
    st.db.call(move |c| fail_running(c, &hub, CUT, now)).await
}

/// Fails the jobs being organized, with `error`, directly: as the server starts without a drive, and
/// as syncing through one is turned on, before its first snapshot (docs/local.md#运行).
pub(crate) fn fail_running(c: &mut Connection, hub: &Hub, error: &str, now: i64) -> Result<(), ApiError> {
    let running: Vec<(String, AiJobId)> = c
        .prepare("SELECT owner_id, id FROM ai_jobs WHERE state = 'running'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (owner, id) in running {
        mutate::set_job_state(c, &actor(&owner), id, JobState::Running, JobState::Failed, Some(error.into()), now)?;
        hub.changed(&owner);
    }
    Ok(())
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

/// A round this device started, as `sync_runs` keeps it: the change that began it, its job, and the
/// job's owner.
pub(crate) type OwnRound = (MutationId, AiJobId, String);

/// The rounds this device started that may still be on.
pub(crate) async fn own_rounds(st: &AppState) -> Result<Vec<OwnRound>, ApiError> {
    st.db
        .call(|c| {
            let mut rows = c.prepare("SELECT run, job_id, (SELECT id FROM users) FROM sync_runs")?;
            Ok(rows.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?)
        })
        .await
}

/// Fails, with `error`, the jobs of `rounds` still in them, through `drive`: rounds whose organizing
/// has ended, as the server stopped, or as syncing did and resumed after stopping failed.
pub(crate) fn cut_rounds(st: &AppState, drive: &Drive, rounds: Vec<OwnRound>, error: &str) {
    for (run, failed) in fail_rounds(drive, rounds, error) {
        let st = st.clone();
        tokio::spawn(async move {
            if failed.await.is_ok() {
                forget(&st, run).await;
            }
        });
    }
}

/// Fails the jobs this device is organizing, as syncing through `drive` stops (docs/local.md#运行),
/// waiting a while only: those whose failure isn't written by then stay as they are on the other
/// devices, to be deleted there.
pub(crate) async fn end_rounds(st: &AppState, drive: &Drive) {
    // A failure to read them is logged as it is made; syncing stops all the same.
    let Ok(rounds) = own_rounds(st).await else { return };
    let failing = fail_rounds(drive, rounds, STOPPED);
    let _ = tokio::time::timeout(drive::WAIT, async {
        for (_, failed) in failing {
            let _ = failed.await;
        }
    })
    .await;
}

/// Queues failing, with `error`, the jobs of `rounds` still in them: each round, and its failure's
/// answer.
fn fail_rounds(
    drive: &Drive,
    rounds: Vec<OwnRound>,
    error: &str,
) -> Vec<(MutationId, impl Future<Output = Result<bool, ApiError>> + use<>)> {
    rounds
        .into_iter()
        .map(|(run, job, owner)| {
            let cut = moved(job, JobState::Running, JobState::Failed, Some(error.to_owned()));
            (run, drive.step(maker(&owner), MutationId(Uuid::now_v7()), within(job, run, cut)))
        })
        .collect()
}

const CUT: &str = "服务器重启，整理中断";
pub(crate) const STOPPED: &str = "停止同步，整理中断";

/// Starts organizing `job` in `turn`; whether it was still waiting to be. Syncing through a drive, it
/// starts once that is in the drive, however long it takes, and the request waits a while only.
async fn start(st: &AppState, turn: Turn, job: AiJobId, config: Config) -> Result<bool, ApiError> {
    let _direct = match st.drive.route().await {
        Route::Drive(drive) => return start_synced(st, drive, turn, job, config).await,
        Route::Direct(held) => held,
    };
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
        tokio::spawn(organize(st.clone(), turn, job, config, Round::Here { since: st.drive.turned_on() }));
    }
    Ok(started)
}

/// Starts organizing `job` through `drive`: once that is in the drive, however long it takes. The
/// request waits a while only.
async fn start_synced(st: &AppState, drive: Arc<Drive>, turn: Turn, job: AiJobId, config: Config) -> Result<bool, ApiError> {
    // Kept before the start is written: should the app stop before the start is in here, the next
    // start of the app knows to end the round.
    let run = MutationId(Uuid::now_v7());
    st.db.call(move |c| Ok(c.execute("INSERT INTO sync_runs (run, job_id) VALUES (?1, ?2)", params![run, job])?)).await?;
    let started = drive.step(maker(&turn.owner), run, moved(job, JobState::Waiting, JobState::Running, None));
    let (tell, told) = oneshot::channel();
    let st = st.clone();
    tokio::spawn(async move {
        let started = started.await;
        let go = matches!(started, Ok(true));
        // The request may have stopped waiting.
        let _ = tell.send(started);
        if go {
            organize(st, turn, job, config, Round::Synced { drive, run }).await;
        } else {
            forget(&st, run).await;
        }
    });
    match tokio::time::timeout(drive::WAIT, told).await {
        Ok(started) => started.expect("the task tells"),
        Err(_) => Err(ApiError::unavailable("暂时连不上网盘，连上后开始整理")),
    }
}

/// Where the changes of one round of organizing a job go: made here at once; or, syncing through a
/// drive, through it, and only while the job is still in the round, which change `run` started
/// (docs/local.md#运行).
enum Round {
    /// Begun without a drive, as syncing had been turned on `since` times: over once it is again,
    /// as the job is failed then.
    Here {
        since: u64,
    },
    Synced {
        drive: Arc<Drive>,
        run: MutationId,
    },
}

/// Makes the change to `owner`'s job `job` that `make` works out, in `round`: whether it applied.
async fn change(st: &AppState, owner: &str, job: AiJobId, round: &Round, make: StepFn) -> Result<bool, ApiError> {
    let since = match round {
        Round::Synced { drive, run } => return drive.step(maker(owner), MutationId(Uuid::now_v7()), within(job, *run, make)).await,
        Round::Here { since } => *since,
    };
    let _direct = match st.drive.route().await {
        Route::Drive(_) => return Ok(false),
        Route::Direct(held) => held,
    };
    // Syncing was turned on since the round began, and the job may be in another device's round now.
    if st.drive.turned_on() != since {
        return Ok(false);
    }
    let (owner, now, hub) = (owner.to_owned(), st.clock.now(), st.hub.clone());
    st.db
        .call(move |c| {
            let tx = c.transaction()?;
            let actor = actor(&owner);
            let Some((ops, done)) = make(&tx, &actor, now)? else { return Ok(false) };
            let recorded = mutate::record(&tx, &actor, MutationId(Uuid::now_v7()), &ops, &done, now)?;
            tx.commit()?;
            hub.notify(&recorded.to);
            Ok(true)
        })
        .await
}

/// `make`, while job `job` is in round `run`: so that a round's late answer does not count in one
/// started since, after the job was deleted and that undone, say.
fn within(job: AiJobId, run: MutationId, make: StepFn) -> StepFn {
    Box::new(move |c, actor, now| {
        let current: Option<MutationId> = c.query_row("SELECT run FROM ai_jobs WHERE id = ?1", [job], |r| r.get(0)).optional()?.flatten();
        if current == Some(run) { make(c, actor, now) } else { Ok(None) }
    })
}

/// Moves job `job` on from state `from` to `to`, with `error`.
fn moved(job: AiJobId, from: JobState, to: JobState, error: Option<String>) -> StepFn {
    Box::new(move |c, actor, now| {
        let moved = |j| AiJob { state: to, error: error.clone(), ..j };
        Ok(mutate::job_change(c, actor, job, from, moved, now)?.map(|(_, done)| (Vec::new(), done)))
    })
}

/// Forgets round `run`, over. Should that fail, the next start of the app ends it again, to no effect.
async fn forget(st: &AppState, run: MutationId) {
    // The error is logged as it is made.
    let _ = st.db.call(move |c| Ok(c.execute("DELETE FROM sync_runs WHERE run = ?1", [run])?)).await;
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
    Organize(organize::Shown),
    Tidy(Box<tidy::Shown>),
}

/// What a job's model answered: tasks, with the refs of the attachments each lists; or suggestions.
enum Answer {
    Tasks(Vec<PlanTask>, Vec<Vec<String>>, organize::Shown),
    Suggestions(Vec<Suggestion>),
}

/// Asks the model and adds what it answers, or keeps its suggestions, or fails the job.
async fn organize(st: AppState, turn: Turn, job: AiJobId, config: Config, round: Round) {
    let _slot = st.ai.slots.acquire().await.expect("the semaphore stays open");
    let began = Instant::now();
    let (owner, vision) = (turn.owner.clone(), config.vision);
    let read = st
        .db
        .call(move |c| {
            let Some(j) = store::load_job(c, &owner, job)?.filter(|j| j.state == JobState::Running) else { return Ok(None) };
            let snap = store::snapshot(c, &owner)?;
            // Organizing reads no task's status, for which the board needs the user's day.
            let board = Board::new(&snap, Now::utc(j.created_at), &owner);
            let notes = notes_along(&board, &store::ai_notes(c, &owner)?, j.folder_id);
            Ok(Some(match j.kind {
                JobKind::Organize => {
                    let mut attached: Vec<&Attachment> = snap.attachments.iter().filter(|a| a.holder == Holder::Job(j.id)).collect();
                    // Ids are UUIDv7, made as the files were added.
                    attached.sort_by_key(|a| a.id);
                    let (prompt, shown, pictures) = organize::prompt(&j, &board, notes, &attached, vision);
                    (prompt, Shown::Organize(shown), pictures)
                }
                JobKind::Tidy => {
                    let (prompt, shown) = tidy::prompt(&j, snap, &owner, notes);
                    (prompt, Shown::Tidy(Box::new(shown)), Vec::new())
                }
            }))
        })
        .await;
    let (mut status, outcome) = match read {
        Ok(None) => return,
        Ok(Some((mut prompt, shown, pictures))) => {
            let (api, files) = (st.ai.api.clone(), st.files.clone());
            let answer = tokio::task::spawn_blocking(move || {
                prompt.pictures = pictures.into_iter().map(|p| p.read(&files)).collect::<std::io::Result<_>>().map_err(|e| {
                    tracing::error!(job = %job, error = %e, "reading a picture for the model");
                    Failure { status: None, message: "读取附件里的图片失败".into() }
                })?;
                api.chat(&config, &prompt, true, TIMEOUT)
            })
            .await
            .expect("chat task panicked");
            match answer {
                Ok(text) => (
                    None,
                    match shown {
                        Shown::Organize(shown) => organize::parse(&text, &shown).map(|(tasks, listed)| Answer::Tasks(tasks, listed, shown)),
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
    let ended = finish(&st, &turn.owner, job, outcome, &round).await;
    if let (Ok(_), Round::Synced { run, .. }) = (&ended, &round) {
        forget(&st, *run).await;
    }
    tracing::info!(job = %job, ended = ?ended.ok(), status, ms = began.elapsed().as_millis(), "ai job");
}

/// Ends a job being organized in `round`: its tasks added and the job deleted, in one change; a
/// tidying ready, with its suggestions; or the job failed.
async fn finish(st: &AppState, owner: &str, job: AiJobId, outcome: Result<Answer, String>, round: &Round) -> Result<Ended, ApiError> {
    let failure = match outcome {
        Err(message) => message,
        Ok(Answer::Suggestions(suggestions)) => {
            let ready: StepFn = Box::new(move |c, actor, now| {
                let ready = |j| AiJob { state: JobState::Ready, suggestions: Some(suggestions.clone()), ..j };
                Ok(mutate::job_change(c, actor, job, JobState::Running, ready, now)?.map(|(_, done)| (Vec::new(), done)))
            });
            return Ok(if change(st, owner, job, round, ready).await? { Ended::Done } else { Ended::Dropped });
        }
        Ok(Answer::Tasks(tasks, listed, shown)) => {
            let add: StepFn = Box::new(move |c, actor, now| added(c, actor, job, &tasks, &listed, &shown, now));
            match change(st, owner, job, round, add).await {
                Ok(added) => return Ok(if added { Ended::Done } else { Ended::Dropped }),
                Err(e) if e.code() == "internal" => return Err(e),
                // Nothing of the plan is kept.
                Err(e) => e.describe(),
            }
        }
    };
    let failed = change(st, owner, job, round, moved(job, JobState::Running, JobState::Failed, Some(failure))).await?;
    Ok(if failed { Ended::Failed } else { Ended::Dropped })
}

/// The tasks job `job`'s model answered added, the job's attachments put under them, and the job
/// deleted; nothing when it is gone or no longer being organized.
fn added(
    c: &Connection,
    actor: &Actor,
    job: AiJobId,
    tasks: &[PlanTask],
    listed: &[Vec<String>],
    shown: &organize::Shown,
    now: i64,
) -> Result<Option<(Vec<Op>, Done)>, ApiError> {
    let owner = actor.user;
    let Some(j) = store::load_job(c, owner, job)?.filter(|j| j.state == JobState::Running) else { return Ok(None) };
    let snap = store::snapshot(c, owner)?;
    let board = Board::new(&snap, Now::utc(now), owner);
    let mut ops = tools::plan_into(&board, j.folder_id, tasks.to_vec(), &shown.tasks)?;
    // The job's attachments go to the tasks they were put under, in the same change; one added since
    // the model was asked, to the first.
    let made: Vec<TaskId> = ops
        .iter()
        .filter_map(|op| match op {
            Op::TaskCreate { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    let under = organize::place(shown.attachments.len(), listed);
    for a in snap.attachments.iter().filter(|a| a.holder == Holder::Job(job)) {
        let task = made[shown.attachments.iter().position(|&s| s == a.id).map_or(0, |i| under[i])];
        ops.push(Op::AttachmentUpdate { id: a.id, name: None, task_id: Some(task) });
    }
    ops.push(Op::AiJobDelete { id: job });
    let done = mutate::run(c, actor, &ops, now)?;
    Ok(Some((ops, done)))
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
    /// As in `Config`.
    #[serde(default)]
    vision: bool,
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
    vision: bool,
    /// The key's last 4 characters; `null` when the server cannot open it, and it has to be given again.
    key_end: Option<String>,
}

fn view(k: &Kept, key: Option<&str>) -> SettingsView {
    SettingsView {
        provider: k.provider.clone(),
        base_url: k.base_url.clone(),
        model: k.model.clone(),
        thinking: k.thinking,
        vision: k.vision,
        key_end: key.map(|k| k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect()),
    }
}

fn checked(c: Config) -> Result<Config, ApiError> {
    c.check().map_err(|m| ApiError::invalid(m, None))?;
    Ok(c)
}

/// The key `given`, or the one kept on the server when none is.
async fn key(st: &AppState, owner: String, given: Option<String>) -> Result<String, ApiError> {
    if let Some(key) = given {
        return Ok(key);
    }
    let ai = st.ai.clone();
    st.db
        .call(move |c| {
            let k = kept(c, &owner)?.ok_or_else(|| ApiError::invalid("请填写 API 密钥", Some("api_key".into())))?;
            k.key(ai.secret.as_ref(), &owner).ok_or_else(|| ApiError::conflict(UNOPENED))
        })
        .await
}

/// The config `input` gives, with the key kept on the server when it gives none.
async fn resolve(st: &AppState, owner: String, input: SettingsInput) -> Result<Config, ApiError> {
    let api_key = key(st, owner, input.api_key).await?;
    checked(Config {
        provider: input.provider,
        base_url: input.base_url,
        model: input.model,
        thinking: input.thinking,
        vision: input.vision,
        api_key,
    })
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
                "INSERT INTO ai_settings (user_id, provider, base_url, model, thinking, vision, api_key, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (user_id) DO UPDATE SET provider = ?2, base_url = ?3, model = ?4, thinking = ?5, vision = ?6, api_key = ?7, updated_at = ?8",
                params![owner, config.provider, config.base_url, config.model, config.thinking, config.vision, sealed, now],
            )?;
            // The queue hears of it too, and starts the jobs that were waiting for settings.
            hub.changed(&owner);
            let k = Kept {
                provider: config.provider,
                base_url: config.base_url,
                model: config.model,
                thinking: config.thinking,
                vision: config.vision,
                api_key: sealed,
            };
            Ok(view(&k, Some(&config.api_key)))
        })
        .await
        .map(Json)
}

/// Of the user's `notes`, those from the root to `folder`, outermost first, as the model is given them.
pub fn notes_along(board: &Board, notes: &HashMap<Option<FolderId>, NoteText>, folder: Option<FolderId>) -> Vec<Value> {
    let mut along = vec![folder];
    while let Some(Some(f)) = along.last() {
        along.push(board.folder(*f).and_then(|f| f.parent_id));
    }
    along.iter().rev().filter_map(|&f| notes.get(&f).map(|text| json!({ "folder": board.path(f), "text": text }))).collect()
}

/// A note for AI organizing (docs/ai.md#说明): the root's, with no folder, is for every folder.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, rename = "AiNote")]
pub struct Note {
    folder_id: Option<FolderId>,
    text: NoteText,
}

/// The user's notes, of the folders they see.
pub async fn get_notes(State(st): State<AppState>, auth: Auth) -> Result<Json<Vec<Note>>, ApiError> {
    auth.require_device()?;
    st.db
        .call(move |c| {
            let seen: HashSet<FolderId> = store::snapshot(c, &auth.user_id)?.folders.iter().map(|f| f.id).collect();
            let notes = store::ai_notes(c, &auth.user_id)?;
            Ok(notes
                .into_iter()
                .filter(|(f, _)| f.is_none_or(|f| seen.contains(&f)))
                .map(|(folder_id, text)| Note { folder_id, text })
                .collect())
        })
        .await
        .map(Json)
}

/// Puts a note in place of the user's for its folder; an empty one clears it.
pub async fn put_note(State(st): State<AppState>, auth: Auth, Json(note): Json<Note>) -> Result<StatusCode, ApiError> {
    auth.require_device()?;
    let _direct = match st.drive.route().await {
        Route::Drive(d) => {
            d.note(auth.into(), note.folder_id, note.text, Some(tokio::time::Instant::now() + drive::WAIT)).await?;
            return Ok(StatusCode::NO_CONTENT);
        }
        Route::Direct(held) => held,
    };
    let now = st.clock.now();
    st.db
        .call(move |c| {
            set_note(c, &auth.user_id, note.folder_id, &note.text, now)?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

/// Puts `text` in place of `user`'s note for `folder`, which they have to see.
pub(crate) fn set_note(c: &Connection, user: &str, folder: Option<FolderId>, text: &NoteText, now: i64) -> Result<(), ApiError> {
    if let Some(f) = folder
        && !store::snapshot(c, user)?.folders.iter().any(|seen| seen.id == f)
    {
        return Err(ApiError::not_found("文件夹不存在"));
    }
    Ok(store::set_ai_note(c, user, folder, text, now)?)
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
    /// Whether the model sees pictures; `null` when the API could not be reached.
    vision: Option<bool>,
}

/// Sends a minimal chat with the settings given, as a job would; then, once that reaches the model,
/// one with a picture.
pub async fn test(State(st): State<AppState>, auth: Auth, Json(input): Json<SettingsInput>) -> Result<Json<TestResult>, ApiError> {
    auth.require_device()?;
    let config = resolve(&st, auth.user_id, input).await?;
    let api = st.ai.api.clone();
    let (answer, secs, vision) = tokio::task::spawn_blocking(move || {
        let began = Instant::now();
        let answer = api.chat(&config, &organize::test_prompt(), true, TEST_TIMEOUT);
        let secs = began.elapsed().as_secs_f32();
        // Many APIs answer a picture they cannot take with an error: the model does not see pictures.
        let vision =
            answer.is_ok().then(|| api.chat(&config, &organize::vision_prompt(), true, TEST_TIMEOUT).is_ok_and(|a| organize::sees_red(&a)));
        (answer, secs, vision)
    })
    .await
    .expect("chat task panicked");
    Ok(Json(match answer {
        Ok(text) if serde_json::from_str::<serde_json::Value>(organize::unwrap(&text)).is_ok() => {
            TestResult { ok: true, message: format!("连接成功，用时 {secs:.1} 秒"), vision }
        }
        Ok(_) => TestResult {
            ok: false, message: "连接成功，但模型没有按要求只输出 JSON，整理时可能失败".into(), vision
        },
        Err(f) => TestResult { ok: false, message: f.message, vision },
    }))
}

#[derive(Serialize, TS)]
#[ts(export, rename = "AiModels")]
pub struct Models {
    models: Vec<String>,
}

/// Where to list the models from; without a key, with the one kept. No model: the list is often
/// what one is picked from.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "AiModelsInput")]
pub struct ModelsInput {
    base_url: String,
    #[ts(optional)]
    api_key: Option<String>,
}

pub async fn models(State(st): State<AppState>, auth: Auth, Json(input): Json<ModelsInput>) -> Result<Json<Models>, ApiError> {
    auth.require_device()?;
    let api_key = key(&st, auth.user_id, input.api_key).await?;
    Endpoint { base_url: &input.base_url, api_key: &api_key }.check().map_err(|m| ApiError::invalid(m, None))?;
    let api = st.ai.api.clone();
    let listed = tokio::task::spawn_blocking(move || api.models(Endpoint { base_url: &input.base_url, api_key: &api_key }))
        .await
        .expect("models task panicked");
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
