//! Syncing devices in local mode through a cloud drive (docs/local.md#借网盘同步). The drive keeps a
//! numbered history of commits, and every device writes them into its database in that order. Every
//! change to the shared data goes through the one task here: it runs the change on the latest data,
//! writes what it did as the next commit unless another device wrote that number first, and only then
//! writes it here, the same way as the commits of other devices.

mod commit;
mod http;
mod remote;
mod replay;
mod s3;
pub mod setup;
mod snapshot;
#[cfg(test)]
mod tests;
mod webdav;

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nexttodo_core::board::Now;
use nexttodo_core::model::{AttachmentId, FolderId, MutationId, NoteText};
use nexttodo_core::ops::{Mutation, Op};
use rusqlite::{Connection, Transaction};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::time::Instant;
use uuid::Uuid;

use crate::auth::Auth;

use crate::error::ApiError;
use crate::files::{Files, Origin};
use crate::mutate::{self, Actor, Applied, Done};
use crate::tools::write::{self, Attempt};
use crate::tools::{Output, WriteFn};
use crate::{AppState, Clock, Db, Hub, MUTATION_RETENTION_MS, ai, backup, db, store};
use commit::{Cleared, Commit, Head, Item, Made, Note};
use remote::{Created, Failed, Remote, remove};

/// How long HTTP and tool requests wait for their change to go in (docs/local.md#排定先后).
pub(crate) const WAIT: Duration = Duration::from_secs(15);
/// How often the drive is looked at for other devices' commits.
const POLL: Duration = Duration::from_secs(30);
/// At most this many items go in one commit.
const BATCH: usize = 50;
/// The wait after a request to the drive failed, doubling up to the longest.
const BACKOFF: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// A snapshot is kept at every commit numbered a multiple of this; a device this many commits behind
/// reads the latest instead of them (docs/local.md#网盘里的文件).
const SNAPSHOT_EVERY: i64 = 200;
const DAY_MS: i64 = 24 * 3600 * 1000;

const NOT_MADE: &str = "暂时连不上网盘，改动没有生效";

/// Whether local mode syncs through a drive now, and through which: turned on and off while the server
/// runs. A change made directly holds `Route::Direct` as it is made, so that a drive turned on waits
/// for it, and has it in its first snapshot.
#[derive(Default)]
pub struct Syncing {
    drive: tokio::sync::RwLock<Option<Arc<Drive>>>,
    /// Why syncing stopped by itself, until it is turned on again.
    stopped: Mutex<Option<String>>,
    /// Held as it is turned on or off in the settings: one request at a time.
    setting: tokio::sync::Mutex<()>,
    /// How many times it was turned on since the server started: a round of AI organizing begun
    /// without a drive is over once it is (docs/local.md#开启).
    turned_on: AtomicU64,
}

/// How a change to the shared data is made now.
pub enum Route<'a> {
    /// Through the drive's sync task.
    Drive(Arc<Drive>),
    /// Directly, holding this meanwhile.
    Direct(tokio::sync::RwLockReadGuard<'a, Option<Arc<Drive>>>),
}

impl Syncing {
    /// Syncs through `drive` from now on, once the changes being made directly are in.
    async fn turn_on(&self, drive: Arc<Drive>) {
        let mut on = self.drive.write().await;
        *self.stopped.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.turned_on.fetch_add(1, Ordering::SeqCst);
        *on = Some(drive);
    }

    /// How many times it was turned on: the same as long as a `Route::Direct` is held.
    pub(crate) fn turned_on(&self) -> u64 {
        self.turned_on.load(Ordering::SeqCst)
    }

    /// No longer syncs: changes are made directly from now on. The sync task has ended by then.
    pub(crate) async fn turn_off(&self, because: Option<String>) {
        let mut on = self.drive.write().await;
        *self.stopped.lock().unwrap_or_else(PoisonError::into_inner) = because;
        *on = None;
    }

    /// The drive it syncs through now, if it does.
    pub async fn current(&self) -> Option<Arc<Drive>> {
        self.drive.read().await.clone()
    }

    /// Why syncing stopped by itself, if it did.
    pub(crate) fn stopped(&self) -> Option<String> {
        self.stopped.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub async fn route(&self) -> Route<'_> {
        let held = self.drive.read().await;
        match (*held).clone() {
            Some(drive) => Route::Drive(drive),
            None => Route::Direct(held),
        }
    }
}

/// The handle to the sync task.
pub struct Drive {
    requests: mpsc::UnboundedSender<Request>,
    wake: Arc<Notify>,
    /// Told each time the task has read in all there was.
    caught_up: Arc<Notify>,
    stop: watch::Sender<bool>,
    status: Arc<Mutex<Status>>,
    /// The local user.
    user: String,
    /// To sync through again, should stopping not go through.
    pub(crate) remote: Arc<dyn Remote>,
}

/// How syncing goes, as the settings show it.
#[derive(Clone, Default, serde::Serialize, ts_rs::TS)]
#[ts(export, rename = "DriveStatus")]
pub struct Status {
    /// The drive's first generation is still being made.
    pub enabling: bool,
    /// When the drive last answered, in milliseconds.
    pub synced_at: Option<i64>,
    /// Why syncing is held up, while it is.
    pub problem: Option<String>,
}

/// Who makes a change: `Actor`, owned.
pub(crate) struct Maker {
    pub user: String,
    pub token_id: String,
    pub via: Option<String>,
}

impl Maker {
    pub(crate) fn actor(&self) -> Actor<'_> {
        Actor { user: &self.user, token_id: &self.token_id, via: self.via.as_deref() }
    }
}

impl From<Auth> for Maker {
    fn from(auth: Auth) -> Self {
        Maker { via: auth.via().map(str::to_owned), user: auth.user_id, token_id: auth.token_id }
    }
}

/// A change waiting to be made.
struct Request {
    maker: Maker,
    work: Work,
    progress: Arc<Progress>,
}

type Answer<T> = Option<oneshot::Sender<Result<T, ApiError>>>;

/// How far a request has got, for the one waiting for it with a deadline: it is not made when they
/// give up before it is sent, and may be once it is (docs/local.md#排定先后). Whichever of the two
/// comes first decides.
#[derive(Default)]
struct Progress(Mutex<Phase>);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Queued,
    Sent,
    GivenUp,
}

impl Progress {
    fn phase(&self) -> std::sync::MutexGuard<'_, Phase> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// As the commit it is in is sent: false when the one waiting has given up on it.
    fn send(&self) -> bool {
        let mut phase = self.phase();
        if *phase == Phase::GivenUp {
            return false;
        }
        *phase = Phase::Sent;
        true
    }

    /// Back in line, as its commit was not written: false when the one waiting has given up on it.
    fn unsend(&self) -> bool {
        let mut phase = self.phase();
        if *phase == Phase::GivenUp {
            return false;
        }
        *phase = Phase::Queued;
        true
    }

    /// The one waiting gives up on it: whether it was sent.
    fn give_up(&self) -> bool {
        std::mem::replace(&mut *self.phase(), Phase::GivenUp) == Phase::Sent
    }

    fn given_up(&self) -> bool {
        *self.phase() == Phase::GivenUp
    }
}

/// A request queued, until it is answered.
struct Queued<T> {
    /// The change it makes, if it is one.
    id: Option<MutationId>,
    answer: oneshot::Receiver<Result<T, ApiError>>,
    progress: Arc<Progress>,
}

impl<T> Queued<T> {
    /// The answer, or with a `deadline`, at the latest then whether it may have been made.
    async fn answered(self, deadline: Option<Instant>) -> Result<T, ApiError> {
        let answer = match deadline {
            None => self.answer.await,
            Some(deadline) => match tokio::time::timeout_at(deadline, self.answer).await {
                Ok(answer) => answer,
                Err(_) if self.progress.give_up() => return Err(ApiError::outcome_unknown(self.id)),
                Err(_) => return Err(ApiError::unavailable(NOT_MADE)),
            },
        };
        // Lost only when the task failed at the database or stopped: a server error, as it would be
        // without a drive.
        answer.unwrap_or_else(|_| Err(ApiError::internal()))
    }
}

/// Works out a change from the data as it is when tried: its operations and what they did, or nothing
/// when it no longer applies. AI organizing's steps are such.
pub(crate) type StepFn = Box<dyn Fn(&Connection, &Actor, i64) -> Result<Option<(Vec<Op>, Done)>, ApiError> + Send + Sync>;

/// What a request is, with the answer it waits for: taken once answered.
enum Work {
    Mutation {
        mutation: Mutation,
        answer: Answer<Applied>,
    },
    /// A write tool, built anew each time it is tried: from the data as it is then.
    Tool {
        id: MutationId,
        build: WriteFn,
        local: Option<Now>,
        answer: Answer<Output>,
    },
    /// Answered whether it applied.
    Step {
        id: MutationId,
        make: StepFn,
        answer: Answer<bool>,
    },
    /// A note for AI organizing.
    Note {
        folder_id: Option<FolderId>,
        text: NoteText,
        answer: Answer<()>,
    },
    /// Clearing the files no attachment has used since before `before`.
    Purge {
        before: i64,
        answer: Answer<()>,
    },
}

impl Request {
    /// The change it makes, as logged; none for a note, which is not.
    fn id(&self) -> Option<MutationId> {
        match &self.work {
            Work::Mutation { mutation, .. } => Some(mutation.id),
            Work::Tool { id, .. } | Work::Step { id, .. } => Some(*id),
            Work::Note { .. } | Work::Purge { .. } => None,
        }
    }

    fn fail(&mut self, e: ApiError) {
        match &mut self.work {
            Work::Mutation { answer, .. } => send(answer, Err(e)),
            Work::Tool { answer, .. } => send(answer, Err(e)),
            Work::Step { answer, .. } => send(answer, Err(e)),
            Work::Note { answer, .. } | Work::Purge { answer, .. } => send(answer, Err(e)),
        }
    }

    /// Answers a tool that only shows what it would do.
    fn show(&mut self, output: Output) {
        let Work::Tool { answer, .. } = &mut self.work else { unreachable!("only tools preview") };
        send(answer, Ok(output));
    }

    /// Answers a request that came to nothing: a step that no longer applies, a purge with nothing to
    /// clear.
    fn moot(&mut self) {
        match &mut self.work {
            Work::Step { answer, .. } => send(answer, Ok(false)),
            Work::Purge { answer, .. } => send(answer, Ok(())),
            Work::Mutation { .. } | Work::Tool { .. } | Work::Note { .. } => unreachable!("they make something or fail"),
        }
    }
}

fn send<T>(answer: &mut Answer<T>, result: Result<T, ApiError>) {
    if let Some(answer) = answer.take() {
        // The one waiting may be gone: a closed connection, or one who gave up.
        let _ = answer.send(result);
    }
}

impl Drive {
    /// Syncs `st`'s data, whose place in the drive's history is in `sync_state`, through `remote` from
    /// now on. `dir` is the data directory.
    pub(crate) async fn start(st: &AppState, dir: PathBuf, remote: Arc<dyn Remote>) -> anyhow::Result<()> {
        let (generation, applied): (String, i64) =
            st.db.hold(|c| c.query_row("SELECT generation, applied FROM sync_state", [], |r| Ok((r.get(0)?, r.get(1)?))))?;
        Self::spawn(st, dir, remote, generation, applied, false).await
    }

    /// Makes `st`'s data the first generation in `remote`'s folder, and syncs through it then
    /// (docs/local.md#开启). Meanwhile changes wait; should it fail, syncing stops.
    pub(crate) async fn enable(st: &AppState, dir: PathBuf, remote: Arc<dyn Remote>) -> anyhow::Result<()> {
        Self::spawn(st, dir, remote, Uuid::now_v7().to_string(), 0, true).await
    }

    /// Turns syncing on before the task runs: the changes being made directly are in when it takes
    /// its first snapshot, and it turns syncing off after, should it stop at once.
    async fn spawn(
        st: &AppState,
        dir: PathBuf,
        remote: Arc<dyn Remote>,
        generation: String,
        applied: i64,
        enabling: bool,
    ) -> anyhow::Result<()> {
        let user: String = st.db.hold(|c| c.query_row("SELECT id FROM users", [], |r| r.get(0)))?;
        let (requests, receiver) = mpsc::unbounded_channel();
        let (wake, (stop, stopping)) = (Arc::new(Notify::new()), watch::channel(false));
        let status = Arc::new(Mutex::new(Status { enabling, ..Status::default() }));
        st.files.set_origin(Some(Arc::new(InDrive { remote: remote.clone(), generation: generation.clone() })));
        let worker = Worker {
            db: st.db.clone(),
            hub: st.hub.clone(),
            clock: st.clock.clone(),
            files: st.files.clone(),
            syncing: st.drive.clone(),
            dir,
            remote: remote.clone(),
            user: user.clone(),
            generation,
            applied,
            checked: false,
            contested: false,
            status: status.clone(),
            caught_up: Arc::new(Notify::new()),
            stop: stopping,
            queue: VecDeque::new(),
            sent: None,
        };
        let (woken, caught_up) = (wake.clone(), worker.caught_up.clone());
        st.drive.turn_on(Arc::new(Drive { requests, wake, caught_up, stop, status, user, remote })).await;
        tokio::spawn(worker.run(receiver, woken, enabling));
        Ok(())
    }

    /// Stops the sync task, and waits for it to end: it writes nothing here after. What waits for it
    /// gets a server error: the page sends its changes again, to be made directly. Files are still
    /// fetched from the drive, until the origin is cleared.
    pub(crate) async fn stop(&self) {
        self.stop.send_replace(true);
        // Closed as the task ends.
        self.requests.closed().await;
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Queues mutation `m`, answered once it is in the drive and here, or turned down. With a
    /// `deadline`, it is given up then: not made if it was not sent yet.
    pub(crate) fn mutate(
        &self,
        maker: Maker,
        mutation: Mutation,
        deadline: Option<Instant>,
    ) -> impl Future<Output = Result<Applied, ApiError>> + use<> {
        self.queue(maker, |answer| Work::Mutation { mutation, answer }).answered(deadline)
    }

    /// Queues write tool `build` as change `id`, as `mutate` does: answered as the tool answers once its
    /// change is in, or at once with what it would do.
    pub(crate) fn tool(
        &self,
        maker: Maker,
        id: MutationId,
        build: WriteFn,
        local: Option<Now>,
        deadline: Option<Instant>,
    ) -> impl Future<Output = Result<Output, ApiError>> + use<> {
        self.queue(maker, |answer| Work::Tool { id, build, local, answer }).answered(deadline)
    }

    /// Queues the change `make` works out, as change `id`, as `mutate` does with no deadline.
    pub(crate) fn step(&self, maker: Maker, id: MutationId, make: StepFn) -> impl Future<Output = Result<bool, ApiError>> + use<> {
        self.queue(maker, |answer| Work::Step { id, make, answer }).answered(None)
    }

    /// Queues putting `text` in place of the note for `folder_id`, as `mutate` does.
    pub(crate) fn note(
        &self,
        maker: Maker,
        folder_id: Option<FolderId>,
        text: NoteText,
        deadline: Option<Instant>,
    ) -> impl Future<Output = Result<(), ApiError>> + use<> {
        self.queue(maker, |answer| Work::Note { folder_id, text, answer }).answered(deadline)
    }

    /// Queues clearing the files no attachment has used since before `before`, on every device.
    pub(crate) fn purge(&self, before: i64) -> impl Future<Output = Result<(), ApiError>> + use<> {
        let maker = Maker { user: self.user.clone(), token_id: String::new(), via: None };
        self.queue(maker, |answer| Work::Purge { before, answer }).answered(None)
    }

    fn queue<T>(&self, maker: Maker, work: impl FnOnce(Answer<T>) -> Work) -> Queued<T> {
        let (answer, receiver) = oneshot::channel();
        let request = Request { maker, work: work(Some(answer)), progress: Arc::new(Progress::default()) };
        let (id, progress) = (request.id(), request.progress.clone());
        // Fails only once the task has stopped; the answer then says so.
        let _ = self.requests.send(request);
        Queued { id, answer: receiver, progress }
    }

    /// Looks at the drive now, as when the app comes to the front.
    pub fn pull(&self) {
        self.wake.notify_one();
    }

    /// Looks at the drive now, and waits up to `wait` for what is new there to be read in, as before
    /// the widget lists the tasks.
    pub async fn caught_up(&self, wait: Duration) {
        let caught_up = self.caught_up.notified();
        tokio::pin!(caught_up);
        caught_up.as_mut().enable();
        self.pull();
        // Not by then: the widget lists what is here.
        let _ = tokio::time::timeout(wait, caught_up).await;
    }
}

/// Why the task can't go on for now.
#[derive(Debug)]
enum Trouble {
    Drive(Failed),
    Here(ApiError),
    /// Something in the drive a newer version of the app wrote: the folder, a commit or a snapshot.
    NewerVersion,
    /// Something in the drive that does not parse.
    Unreadable(String),
    /// `nexttodo.json` no longer points to this generation: the folder was deleted, or made anew by
    /// another device. Syncing stops.
    Replaced,
    /// Another device made the first generation as this one did. Syncing stops.
    Taken,
    /// Syncing was stopped in the settings.
    Stopped,
}

impl Trouble {
    /// What the user is told of it.
    fn message(&self) -> String {
        match self {
            Trouble::Stopped => "同步已停止".into(),
            Trouble::Drive(f) => f.message().to_owned(),
            Trouble::Here(e) => format!("本机的数据出错：{e}"),
            Trouble::NewerVersion => "网盘里有更新版本的 NextTodo 写的数据，请先更新 App".into(),
            Trouble::Unreadable(why) => format!("网盘里的数据无法读取：{why}"),
            Trouble::Replaced => "网盘里的文件夹换过了，请重新开启同步".into(),
            Trouble::Taken => "别的设备刚刚开启了同步，请重新开启：会用网盘里的数据替换这台设备上的全部数据".into(),
        }
    }
}

impl From<Failed> for Trouble {
    fn from(f: Failed) -> Self {
        Trouble::Drive(f)
    }
}

impl From<ApiError> for Trouble {
    fn from(e: ApiError) -> Self {
        Trouble::Here(e)
    }
}

struct Worker {
    db: Db,
    hub: Hub,
    clock: Clock,
    files: Arc<Files>,
    /// To turn itself off with, when it has to stop.
    syncing: Arc<Syncing>,
    /// The data directory.
    dir: PathBuf,
    remote: Arc<dyn Remote>,
    user: String,
    generation: String,
    /// The number of the last commit written here.
    applied: i64,
    /// `nexttodo.json` was found to point to this generation since the task started.
    checked: bool,
    /// Another device wrote the number this one tried: the next read is to find its commit.
    contested: bool,
    status: Arc<Mutex<Status>>,
    caught_up: Arc<Notify>,
    /// Set to stop: the task stops at the next request to the drive, or before the next thing it
    /// does, never as it writes here.
    stop: watch::Receiver<bool>,
    /// Waiting to be made, in the order they came.
    queue: VecDeque<Request>,
    /// A commit sent for the next number and not yet written here: in the drive there, or not.
    sent: Option<Sent>,
}

struct Sent {
    id: Uuid,
    body: Vec<u8>,
    requests: Vec<Request>,
}

/// A commit read in: its id, and the files it cleared.
struct ReadIn {
    id: Uuid,
    cleared: Vec<AttachmentId>,
}

impl Worker {
    async fn run(mut self, mut requests: mpsc::UnboundedReceiver<Request>, wake: Arc<Notify>, enabling: bool) {
        if enabling {
            match self.enable().await {
                Ok(()) => self.report(|s| s.enabling = false),
                Err(Trouble::Stopped) => return self.end(requests),
                Err(trouble) => {
                    tracing::warn!(?trouble, "enabling sync failed");
                    return self.give_up(trouble, requests).await;
                }
            }
        }
        let mut backoff = BACKOFF;
        loop {
            if *self.stop.borrow() {
                return self.end(requests);
            }
            while let Ok(r) = requests.try_recv() {
                self.queue.push_back(r);
            }
            // Told they were not made.
            self.queue.retain(|r| !r.progress.given_up());
            let stepped = self.step().await;
            let now = self.clock.now();
            let wait = match stepped {
                Ok(more) => {
                    self.report(|s| (s.synced_at, s.problem) = (Some(now), None));
                    backoff = BACKOFF;
                    if more {
                        continue;
                    }
                    self.caught_up.notify_waiters();
                    POLL
                }
                Err(Trouble::Stopped) => return self.end(requests),
                Err(trouble @ (Trouble::Replaced | Trouble::Taken)) => return self.give_up(trouble, requests).await,
                Err(trouble) => {
                    tracing::warn!(?trouble, "sync with the drive held up");
                    self.report(|s| s.problem = Some(trouble.message()));
                    let wait = if matches!(trouble, Trouble::NewerVersion | Trouble::Unreadable(_)) { POLL } else { backoff };
                    backoff = (backoff * 2).min(BACKOFF_MAX);
                    wait
                }
            };
            tokio::select! {
                r = requests.recv() => match r {
                    Some(r) => self.queue.push_back(r),
                    None => return self.end(requests),
                },
                () = wake.notified() => {}
                () = self.stopping() => return self.end(requests),
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    fn report(&self, change: impl FnOnce(&mut Status)) {
        change(&mut self.status.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Done once the task is to stop.
    fn stopping(&self) -> impl Future<Output = ()> + use<> {
        let mut stop = self.stop.clone();
        async move {
            // An error: the handle is gone, and so is the reason to go on.
            let _ = stop.wait_for(|stop| *stop).await;
        }
    }

    /// Waits for `wait`, unless the task is to stop first.
    async fn pause(&self, wait: Duration) -> Result<(), Trouble> {
        tokio::select! {
            () = tokio::time::sleep(wait) => Ok(()),
            () = self.stopping() => Err(Trouble::Stopped),
        }
    }

    /// Ends the task: whatever waits for it gets a server error, the page sends its changes again.
    /// `Drive::stop` waits for the requests to close.
    fn end(mut self, mut requests: mpsc::UnboundedReceiver<Request>) {
        requests.close();
        while let Ok(r) = requests.try_recv() {
            self.queue.push_back(r);
        }
        for mut r in self.queue.drain(..).chain(self.sent.take().into_iter().flat_map(|s| s.requests)) {
            r.fail(ApiError::internal());
        }
    }

    /// Stops syncing for `trouble`: told in the settings until it is turned on again. What waits is
    /// answered once syncing is off, so that what is sent again is made directly.
    async fn give_up(self, trouble: Trouble, requests: mpsc::UnboundedReceiver<Request>) {
        let because = trouble.message();
        if matches!(trouble, Trouble::Taken) {
            // What it uploaded of its own generation; what is left is cleared with the generations
            // `nexttodo.json` does not point to.
            let generation = self.generation.clone();
            let _ = self.call(move |r| remove(r, &generation)).await;
        }
        if let Err(e) = setup::forget(&self.db, &self.dir).await {
            tracing::error!("forgetting the drive: {e}");
        }
        // Before syncing is off, after which it may be turned on again, with a drive of its own.
        self.files.set_origin(None);
        self.syncing.turn_off(Some(because)).await;
        self.end(requests);
    }

    /// Does the next thing: settles a commit sent without an answer, reads the next commit of
    /// another device, or writes what is waiting. Whether there is more to do at once.
    async fn step(&mut self) -> Result<bool, Trouble> {
        if let Some(sent) = self.sent.take() {
            return self.settle(sent).await;
        }
        let synced_at = self.status.lock().unwrap_or_else(PoisonError::into_inner).synced_at;
        if synced_at.is_none_or(|at| self.clock.now() - at > DAY_MS) {
            // Far behind, as after long offline, the latest snapshot is quicker to read than the
            // commits since, and is there when they were deleted.
            let dir = format!("{}/snapshots", self.generation);
            let newest = self.call(move |r| Ok(snapshots(&r.list(&dir)?).last().copied())).await?;
            if let Some(number) = newest.filter(|n| *n >= self.applied + SNAPSHOT_EVERY) {
                self.swap(number).await?;
                return Ok(true);
            }
        }
        if let Some(body) = self.get(commit::path(&self.generation, self.applied + 1)).await? {
            self.contested = false;
            self.apply(&body, &[]).await?;
            return Ok(true);
        }
        if std::mem::take(&mut self.contested) {
            // Written by another device, and not there: a folder deleted, say. Held off for a while,
            // rather than asked again at once.
            return Err(Failed::Unknown("网盘的回答前后矛盾".into()).into());
        }
        // Nothing is written into a generation that is no longer the one: on an object store, writing
        // into a folder deleted goes through. A device that only reads finds out as it starts.
        if self.queue.is_empty() {
            if !self.checked {
                self.check().await?;
            }
            return Ok(false);
        }
        self.check().await?;
        self.write().await
    }

    /// Makes the first changes waiting on the latest data, and writes what they did as the next
    /// commit.
    async fn write(&mut self) -> Result<bool, Trouble> {
        let n = self.queue.len().min(BATCH);
        let requests: Vec<Request> = self.queue.drain(..n).collect();
        let (now, files) = (self.clock.now(), self.files.clone());
        let (requests, tried) = self
            .db
            .call(move |c| {
                let mut tx = c.transaction()?;
                let tried: Vec<_> = requests.iter().map(|r| try_out(&mut tx, r, &files, now)).collect();
                // Rolled back: it is written here once it is in the drive.
                drop(tx);
                Ok((requests, tried))
            })
            .await?;
        let (mut items, mut kept) = (Vec::new(), Vec::new());
        // For each item, its request's place in `kept`.
        let mut makers = Vec::new();
        for (mut r, tried) in requests.into_iter().zip(tried) {
            match tried {
                Err(e) => r.fail(e),
                Ok(Tried::Preview(output)) => r.show(output),
                Ok(Tried::Moot) => r.moot(),
                Ok(Tried::Made(item)) => {
                    makers.push(kept.len());
                    items.push(item);
                    kept.push(r);
                }
                // Here already, or one earlier in this commit.
                Ok(Tried::Before) => kept.push(r),
            }
        }
        if items.is_empty() {
            self.answer(kept).await?;
            return Ok(true);
        }
        match self.upload(&items).await {
            Ok(None) => {}
            Ok(Some(item)) => {
                kept.remove(makers[item]).fail(ApiError::conflict("网盘里已经有另一个文件用了这个附件 ID"));
                // Made again without it: what they did may have rested on it.
                self.requeue(kept);
                return Ok(true);
            }
            Err(trouble) => {
                self.requeue(kept);
                return Err(trouble);
            }
        }
        // Each is sent from now on, unless the one waiting gave up meanwhile: then made again
        // without it.
        if !kept.iter().all(|r| r.progress.send()) {
            self.requeue(kept);
            return Ok(true);
        }
        let commit = Commit { id: Uuid::now_v7(), format: commit::FORMAT, items };
        let body = serde_json::to_vec(&commit).expect("commits serialize");
        let sent = Sent { id: commit.id, body, requests: kept };
        self.create(sent).await
    }

    /// Writes `sent` at the next number, unless another commit is there.
    async fn create(&mut self, sent: Sent) -> Result<bool, Trouble> {
        let path = commit::path(&self.generation, self.applied + 1);
        let body = sent.body.clone();
        match self.call(move |r| r.create(&path, &body)).await {
            Ok(Created::Written) => {
                let read = match self.apply(&sent.body, &sent.requests).await {
                    Ok(read) => read,
                    // It is in the drive: read from there again.
                    Err(trouble) => {
                        self.sent = Some(sent);
                        return Err(trouble);
                    }
                };
                self.wrote(sent.requests, read.cleared).await?;
                Ok(true)
            }
            // Another device was first: read its commit, then try again on what it did.
            Ok(Created::Exists) => {
                self.contested = true;
                self.requeue(sent.requests);
                Ok(true)
            }
            Err(unsent @ Trouble::Drive(Failed::Unsent(_))) => {
                self.requeue(sent.requests);
                Err(unsent)
            }
            Err(trouble) => {
                self.sent = Some(sent);
                Err(trouble)
            }
        }
    }

    /// Finds out whether `sent` was written: whatever is at its number is read; when nothing is,
    /// the very same commit is sent again, so that the first one, should it still arrive, makes no
    /// difference. That too only into the generation that is the one.
    async fn settle(&mut self, sent: Sent) -> Result<bool, Trouble> {
        let path = commit::path(&self.generation, self.applied + 1);
        let found = match self.get(path).await {
            Ok(None) => self.check().await.map(|()| None),
            found => found,
        };
        let found = match found {
            Ok(found) => found,
            Err(trouble) => {
                self.sent = Some(sent);
                return Err(trouble);
            }
        };
        let Some(body) = found else { return self.create(sent).await };
        match self.apply(&body, &sent.requests).await {
            Ok(read) if read.id == sent.id => self.wrote(sent.requests, read.cleared).await?,
            Ok(_) => self.requeue(sent.requests),
            Err(trouble) => {
                self.sent = Some(sent);
                return Err(trouble);
            }
        }
        Ok(true)
    }

    /// What follows a commit of this device's going in, here and in the drive: the answers, deleting
    /// the files it cleared from the drive, and every so many commits a snapshot.
    async fn wrote(&self, requests: Vec<Request>, cleared: Vec<AttachmentId>) -> Result<(), Trouble> {
        self.answer(requests).await?;
        self.unstore(cleared).await;
        if self.applied % SNAPSHOT_EVERY == 0 {
            self.keep_snapshot().await;
        }
        Ok(())
    }

    /// Takes a snapshot of the data at this number, and has it put into the drive meanwhile, and what
    /// is no longer needed there cleared after (docs/local.md#网盘里的文件). When that fails, joining
    /// devices and those far behind read an earlier one.
    async fn keep_snapshot(&self) {
        let (db, dir) = (self.db.clone(), self.dir.join("sync"));
        let taken = tokio::task::spawn_blocking(move || snapshot::take(&db, &dir)).await.expect("taking a snapshot panicked");
        let bytes = match taken {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("taking a snapshot: {e:#}");
                return;
            }
        };
        let (remote, generation, number, now) = (self.remote.clone(), self.generation.clone(), self.applied, self.clock.now());
        tokio::task::spawn_blocking(move || {
            let kept = remote
                .create(&format!("{generation}/snapshots/{number}.db.gz"), &bytes)
                .and_then(|_| clear(remote.as_ref(), &generation, now));
            if let Err(f) = kept {
                tracing::warn!(error = f.message(), number, "keeping a snapshot in the drive");
            }
        });
    }

    /// Puts back requests that were not written, first in line; those given up on, told they may
    /// have been made, are not made after all.
    fn requeue(&mut self, requests: Vec<Request>) {
        for r in requests.into_iter().rev().filter(|r| r.progress.unsend()) {
            self.queue.push_front(r);
        }
    }

    /// Puts the files of the attachments `items` add into the drive, those here: one not here was
    /// attached before, and is there already. The item, if any, whose file another is there in place
    /// of.
    async fn upload(&self, items: &[Item]) -> Result<Option<usize>, Trouble> {
        for (i, item) in items.iter().enumerate() {
            for id in item.attached() {
                if !self.upload_file(id).await? {
                    return Ok(Some(i));
                }
            }
        }
        Ok(None)
    }

    /// Puts attachment `id`'s file into the drive, when it is here: whether what is there is it.
    async fn upload_file(&self, id: AttachmentId) -> Result<bool, Trouble> {
        let files = self.files.clone();
        let here = tokio::task::spawn_blocking(move || files.here(id)).await.expect("reading a file panicked").map_err(ApiError::from)?;
        let Some(bytes) = here else { return Ok(true) };
        let path = commit::file_path(&self.generation, id);
        self.call(move |r| match r.create(&path, &bytes)? {
            Created::Written => Ok(true),
            // There from an earlier try, or another device's file under the same id.
            Created::Exists => Ok(r.get(&path)?.is_some_and(|there| there == bytes)),
        })
        .await
    }

    /// Reads snapshot `number` in place of the commits up to it (docs/local.md#运行).
    async fn swap(&mut self, number: i64) -> Result<(), Trouble> {
        let path = format!("{}/snapshots/{number}.db.gz", self.generation);
        // Deleted since it was listed: two newer ones were kept meanwhile.
        let bytes = self.get(path).await?.ok_or_else(|| Trouble::Drive(Failed::Unknown("网盘里的快照刚被清理".into())))?;
        let dir = self.dir.join("sync");
        let copy = dir.join(format!("swap-{}.db", Uuid::now_v7()));
        let to = copy.clone();
        let unpacked = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&dir)?;
            snapshot::unpack(&bytes, &to)?;
            backup::convert(&to, None)
        })
        .await
        .expect("unpacking a snapshot panicked");
        let (user, hub, from) = (self.user.clone(), self.hub.clone(), copy.clone());
        let swapped = match unpacked {
            Ok(()) => self
                .db
                .call(move |c| {
                    snapshot::swap(c, &from, number)?;
                    hub.changed(&user);
                    Ok(())
                })
                .await
                .map_err(Trouble::from),
            Err(e) if e.is::<db::Newer>() => Err(Trouble::NewerVersion),
            Err(e) => Err(Trouble::Unreadable(format!("{e:#}"))),
        };
        // What failed is what to tell.
        let _ = std::fs::remove_file(&copy);
        swapped?;
        tracing::info!(from = self.applied, to = number, "read the latest snapshot in place of the commits");
        self.applied = number;
        Ok(())
    }

    /// Whether `nexttodo.json` points to this generation.
    async fn check(&mut self) -> Result<(), Trouble> {
        match self.get(setup::POINTER.into()).await?.map(|b| setup::Pointer::read(&b)).transpose()? {
            Some(p) if p.id == self.generation => {
                self.checked = true;
                Ok(())
            }
            _ => Err(Trouble::Replaced),
        }
    }

    /// Makes the first generation from the data here, as the changes coming meanwhile wait: its
    /// folders, the files of the attachments, snapshot 0, and last `nexttodo.json`, which makes it the
    /// one (docs/local.md#开启). Then it is this device's place in the drive's history.
    async fn enable(&mut self) -> Result<(), Trouble> {
        let g = self.generation.clone();
        for dir in [String::new(), "tmp".into(), g.clone(), format!("{g}/log"), format!("{g}/files"), format!("{g}/snapshots")] {
            self.call(move |r| r.mkdir(&dir)).await?;
        }
        // Organized from here on, they would be in no round the drive keeps track of (docs/local.md#运行).
        let (hub, now) = (self.hub.clone(), self.clock.now());
        self.db.call(move |c| ai::fail_running(c, &hub, "开启同步，整理中断", now)).await?;
        let (db, dir) = (self.db.clone(), self.dir.join("sync"));
        let taken = tokio::task::spawn_blocking(move || snapshot::take(&db, &dir)).await.expect("taking a snapshot panicked");
        let snapshot = taken.map_err(|e| {
            tracing::error!("taking a snapshot: {e:#}");
            Trouble::Here(ApiError::internal())
        })?;
        // What the snapshot has: nothing changes the shared data meanwhile.
        let ids: Vec<AttachmentId> = self
            .db
            .call(|c| {
                Ok(c.prepare("SELECT id FROM files WHERE NOT staged AND NOT purged")?
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<_, _>>()?)
            })
            .await?;
        for id in ids {
            // A generation's first: whatever is there was put there by this device.
            self.upload_file(id).await?;
        }
        let path = format!("{g}/snapshots/0.db.gz");
        self.call(move |r| r.create(&path, &snapshot)).await?;
        // Before `nexttodo.json` may point to it: cut short after, the next start syncs on with it, or
        // finds it is not the one and stops.
        let g = self.generation.clone();
        self.db.call(move |c| Ok(c.execute("INSERT INTO sync_state (id, generation, applied) VALUES (1, ?1, 0)", [g])?)).await?;
        let pointer = setup::pointer(&self.generation);
        loop {
            let body = pointer.clone();
            match self.call(move |r| r.create(setup::POINTER, &body)).await {
                Ok(Created::Written) => break,
                // Whose it is, if it is there, says whether this one was written.
                Ok(Created::Exists) | Err(Trouble::Drive(Failed::Unknown(_))) => match self.get(setup::POINTER.into()).await {
                    Ok(Some(found)) if found == pointer => break,
                    Ok(Some(_)) => return Err(Trouble::Taken),
                    Ok(None) => {}
                    Err(Trouble::Stopped) => return Err(Trouble::Stopped),
                    Err(_) => self.pause(BACKOFF_MAX).await?,
                },
                Err(trouble) => return Err(trouble),
            }
        }
        self.checked = true;
        Ok(())
    }

    /// Deletes the files a commit of this device's cleared from the drive, after every device reading
    /// it deleted their own. One that can't be deleted now stays there, unused.
    async fn unstore(&self, cleared: Vec<AttachmentId>) {
        for id in cleared {
            let path = commit::file_path(&self.generation, id);
            if let Err(trouble) = self.call(move |r| r.delete(&path)).await {
                tracing::warn!(?trouble, "deleting a cleared file from the drive");
                return;
            }
        }
    }

    /// Writes commit `body`, the next one, here. `mine` are the requests it may hold: their changes
    /// are logged with their tokens, and a tool's answer is kept with its change, as when made without
    /// a drive.
    async fn apply(&mut self, body: &[u8], mine: &[Request]) -> Result<ReadIn, Trouble> {
        let commit = parse(body)?;
        let tokens: HashMap<MutationId, String> = mine.iter().filter_map(|r| Some((r.id()?, r.maker.token_id.clone()))).collect();
        let tools: HashMap<MutationId, Option<Now>> = mine
            .iter()
            .filter_map(|r| match &r.work {
                Work::Tool { id, local, .. } => Some((*id, local.clone())),
                Work::Mutation { .. } | Work::Step { .. } | Work::Note { .. } | Work::Purge { .. } => None,
            })
            .collect();
        let (user, number, hub, files, id) = (self.user.clone(), self.applied + 1, self.hub.clone(), self.files.clone(), commit.id);
        let cleared = self
            .db
            .call(move |c| {
                let cleared = read_in(c, &user, commit, number, &tokens, &tools)?;
                for id in &cleared {
                    // One that can't be deleted now is left to the sweep.
                    let _ = files.remove(*id);
                }
                hub.changed(&user);
                Ok(cleared)
            })
            .await?;
        self.applied = number;
        Ok(ReadIn { id, cleared })
    }

    /// Answers requests whose changes are here, as made.
    async fn answer(&self, requests: Vec<Request>) -> Result<(), Trouble> {
        const HERE: &str = "a change just written is here";
        self.db
            .call(move |c| {
                for mut r in requests {
                    let actor = r.maker.actor();
                    match &mut r.work {
                        Work::Mutation { mutation, answer } => send(answer, Ok(mutate::earlier(c, &actor, mutation.id)?.expect(HERE))),
                        Work::Tool { id, answer, .. } => send(answer, Ok(write::earlier(c, &actor, *id)?.expect(HERE))),
                        Work::Step { answer, .. } => send(answer, Ok(true)),
                        Work::Note { answer, .. } | Work::Purge { answer, .. } => send(answer, Ok(())),
                    }
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    async fn get(&self, path: String) -> Result<Option<Vec<u8>>, Trouble> {
        self.call(move |r| r.get(&path)).await
    }

    /// Makes request `f` to the drive, unless the task is to stop first.
    async fn call<T: Send + 'static>(&self, f: impl FnOnce(&dyn Remote) -> Result<T, Failed> + Send + 'static) -> Result<T, Trouble> {
        let remote = self.remote.clone();
        let request = tokio::task::spawn_blocking(move || f(remote.as_ref()));
        tokio::select! {
            done = request => Ok(done.expect("drive request panicked")?),
            // Not waited for: should it write a commit, the other devices read it in, and here the
            // page sends the change again.
            () = self.stopping() => Err(Trouble::Stopped),
        }
    }
}

/// The numbers of the snapshots among `names`, a generation's `snapshots/`, in order.
fn snapshots(names: &[String]) -> Vec<i64> {
    let mut numbers: Vec<i64> = names.iter().filter_map(|n| n.strip_suffix(".db.gz")?.parse().ok()).collect();
    numbers.sort_unstable();
    numbers
}

/// Whether `name` is that of a generation's folder, or of anything NextTodo names by a UUIDv7.
fn generation(name: &str) -> bool {
    Uuid::parse_str(name).ok().and_then(made_at).is_some()
}

/// When UUIDv7 `id` was made, in milliseconds.
fn made_at(id: Uuid) -> Option<i64> {
    let (secs, nanos) = id.get_timestamp()?.to_unix();
    Some(i64::try_from(secs).ok()? * 1000 + i64::from(nanos / 1_000_000))
}

/// Clears what generation `generation` of the drive no longer needs, at `now` (docs/local.md#网盘里的文件).
fn clear(r: &dyn Remote, generation: &str, now: i64) -> Result<(), Failed> {
    let numbers = snapshots(&r.list(&format!("{generation}/snapshots"))?);
    // The latest two stay; one before them goes with the commits before it, once old enough.
    for &number in &numbers[..numbers.len().saturating_sub(2)] {
        let head = r.get(&commit::path(generation, number))?;
        if head
            .as_deref()
            .and_then(|b| serde_json::from_slice::<Head>(b).ok())
            .and_then(|h| made_at(h.id))
            .is_some_and(|at| now - at < MUTATION_RETENTION_MS)
        {
            break;
        }
        // The commits it stands for, `SNAPSHOT_EVERY` at a time back to those cleared already. Each
        // lot's last goes last, so one there has its lot still to clear: cut short, or of a snapshot
        // that was never kept.
        let (mut last, mut there) = (number, head.is_some());
        while there {
            for n in last - SNAPSHOT_EVERY + 1..=last {
                r.delete(&commit::path(generation, n))?;
            }
            last -= SNAPSHOT_EVERY;
            there = last > 0 && r.get(&commit::path(generation, last))?.is_some();
        }
        r.delete(&format!("{generation}/snapshots/{number}.db.gz"))?;
    }
    // Left by writes and by enabling cut short, or lost to another device.
    let stale = |name: &str| Uuid::parse_str(name).ok().and_then(made_at).is_some_and(|at| now - at > DAY_MS);
    for name in r.list("tmp")? {
        if stale(&name) {
            r.delete(&format!("tmp/{name}"))?;
        }
    }
    // Only while this generation is the one: another may be newer than this device knows.
    let pointer = r.get(setup::POINTER)?.and_then(|b| setup::Pointer::read(&b).ok());
    if pointer.is_some_and(|p| p.id == generation) {
        for name in r.list("")? {
            if name != generation && stale(&name) {
                remove(r, &name)?;
            }
        }
    }
    Ok(())
}

/// A commit as read from the drive: one of a newer format, or one that does not parse, is not read.
fn parse(body: &[u8]) -> Result<Commit, Trouble> {
    let head: Head = serde_json::from_slice(body).map_err(|e| Trouble::Unreadable(e.to_string()))?;
    if head.format > commit::FORMAT {
        return Err(Trouble::NewerVersion);
    }
    serde_json::from_slice(body).map_err(|e| Trouble::Unreadable(e.to_string()))
}

/// Writes `commit`, number `number`, here in one transaction, as docs/local.md#读入 has it, for `user`:
/// `tokens` and `tools` are what this device knows of the changes in it it made, their tokens and,
/// for a tool, the day it was made on, to answer with. Returns the files it cleared.
fn read_in(
    c: &mut Connection,
    user: &str,
    commit: Commit,
    number: i64,
    tokens: &HashMap<MutationId, String>,
    tools: &HashMap<MutationId, Option<Now>>,
) -> Result<Vec<AttachmentId>, ApiError> {
    let tx = c.transaction()?;
    let mut cleared = Vec::new();
    for item in commit.items {
        match item {
            Item::Mutation(m) => {
                let actor = Actor { user, token_id: tokens.get(&m.id).map_or("", String::as_str), via: m.via.as_deref() };
                let reply = tools.get(&m.id).map(|local| (local.clone(), m.effects.clone()));
                let seq = replay::mutation(&tx, &actor, m.id, m.at, &m.ops, m.effects)?.seq;
                if let Some((local, effects)) = reply {
                    write::reported(&tx, user, m.at, local, seq, &effects)?;
                }
            }
            Item::Note(n) => store::set_ai_note(&tx, user, n.folder_id, &n.text, n.at)?,
            Item::Purge(gone) => {
                replay::purge(&tx, user, &gone)?;
                cleared.extend(gone.iter().map(|f| f.id));
            }
        }
    }
    tx.execute("UPDATE sync_state SET applied = ?1", [number])?;
    tx.commit()?;
    Ok(cleared)
}

/// What trying out a request came to.
enum Tried {
    /// What it did, kept in the transaction for the next ones to see.
    Made(Item),
    /// Its change was made before.
    Before,
    /// A tool that only shows what it would do.
    Preview(Output),
    /// A step that no longer applies.
    Moot,
}

fn try_out(tx: &mut Transaction, r: &Request, files: &Files, now: i64) -> Result<Tried, ApiError> {
    let actor = r.maker.actor();
    let point = tx.savepoint()?;
    let id = match &r.work {
        Work::Note { folder_id, text, .. } => {
            ai::set_note(&point, actor.user, *folder_id, text, now)?;
            point.commit()?;
            return Ok(Tried::Made(Item::Note(Note { folder_id: *folder_id, text: text.clone(), at: now })));
        }
        Work::Purge { before, .. } => {
            let cleared = purgeable(&point, *before)?;
            if cleared.is_empty() {
                return Ok(Tried::Moot);
            }
            replay::purge(&point, actor.user, &cleared)?;
            point.commit()?;
            return Ok(Tried::Made(Item::Purge(cleared)));
        }
        Work::Mutation { mutation, .. } => mutation.id,
        Work::Tool { id, .. } | Work::Step { id, .. } => *id,
    };
    if mutate::earlier(&point, &actor, id)?.is_some() {
        return Ok(Tried::Before);
    }
    let (ops, done) = match &r.work {
        Work::Mutation { mutation, .. } => (mutation.ops.clone(), mutate::run(&point, &actor, &mutation.ops, now)?),
        Work::Tool { build, local, .. } => match write::attempt(&point, &actor, files, now, local.clone(), build)? {
            Attempt::Preview(output) => return Ok(Tried::Preview(output)),
            Attempt::Made { ops, done } => (ops, done),
        },
        Work::Step { make, .. } => match make(&point, &actor, now)? {
            Some(made) => made,
            None => return Ok(Tried::Moot),
        },
        Work::Note { .. } | Work::Purge { .. } => unreachable!("made above"),
    };
    mutate::record(&point, &actor, id, &ops, &done, now)?;
    point.commit()?;
    Ok(Tried::Made(Item::Mutation(Made { id, at: now, via: r.maker.via.clone(), ops, effects: done.effects() })))
}

/// The files no attachment has used since before `before`, and those uploaded then and never
/// attached.
fn purgeable(c: &Connection, before: i64) -> rusqlite::Result<Vec<Cleared>> {
    c.prepare("SELECT id, type, size, created_at, unused_at FROM files WHERE unused_at < ?1 AND NOT purged ORDER BY id")?
        .query_map([before], |r| {
            Ok(Cleared { id: r.get(0)?, media_type: r.get(1)?, size: r.get(2)?, created_at: r.get(3)?, unused_at: r.get(4)? })
        })?
        .collect()
}

/// The files of attachments in the drive, for those not here yet.
struct InDrive {
    remote: Arc<dyn Remote>,
    generation: String,
}

impl Origin for InDrive {
    fn fetch(&self, id: AttachmentId) -> io::Result<Option<Vec<u8>>> {
        self.remote.get(&commit::file_path(&self.generation, id)).map_err(|f| io::Error::other(f.message().to_owned()))
    }
}
