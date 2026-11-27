//! The sync task, with several devices on one drive in memory.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use nexttodo_core::model::{AiJob, AiJobId, AttachmentId, FileSize, JobState};

use nexttodo_core::model::MutationId;
use nexttodo_core::ops::Mutation;
use rusqlite::types::Value;
use serde_json::json;
use tokio::time::Instant;
use uuid::Uuid;

use super::remote::Remote;
use super::remote::memory::{Fault, Store, View};
use super::{DAY_MS, Drive, Maker, NOT_MADE, StepFn, Synced, Trouble, setup};
use crate::ai::llm::{Api, Config, Endpoint, Failure, Prompt};
use crate::ai::{self, Ai};
use crate::auth::{Auth, Scope, TokenKind};
use crate::error::{ApiError, Json};
use crate::files::Files;
use crate::mutate::{self, Applied};
use crate::tools::{self, Reply};
use crate::{AppState, Clock, Db, backup};

pub(crate) fn id() -> String {
    Uuid::now_v7().to_string()
}

/// `n` databases with the same local user, and the user's id.
pub(crate) fn same_user(n: usize) -> (Vec<Db>, String) {
    let dbs: Vec<Db> = (0..n).map(|_| Db::open_in_memory().unwrap()).collect();
    let user = dbs[0].hold(|c| crate::local::local_user(c, 1).unwrap());
    let row: Vec<Value> = dbs[0].hold(|c| {
        let mut s = c.prepare("SELECT * FROM users").unwrap();
        let n = s.column_count();
        s.query_row([], |r| (0..n).map(|i| r.get(i)).collect()).unwrap()
    });
    for db in &dbs[1..] {
        db.hold(|c| {
            let marks = vec!["?"; row.len()].join(", ");
            c.execute(&format!("INSERT INTO users VALUES ({marks})"), rusqlite::params_from_iter(&row)).unwrap();
        });
    }
    (dbs, user)
}

pub(crate) fn dump(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    db.hold(|c| {
        let mut s = c.prepare(sql).unwrap();
        let n = s.column_count();
        s.query_map([], |r| (0..n).map(|i| r.get(i)).collect()).unwrap().collect::<Result<_, _>>().unwrap()
    })
}

/// Everything devices share (docs/local/drive.md#共享的和本机的).
pub(crate) const SHARED: &[&str] = &[
    "SELECT * FROM folders ORDER BY id",
    "SELECT * FROM tasks ORDER BY id",
    "SELECT * FROM dependencies ORDER BY before_id, after_id",
    "SELECT * FROM completions ORDER BY task_id, due_date",
    "SELECT * FROM attachments ORDER BY id",
    "SELECT id, owner_id, type, size, unused_at, purged FROM files WHERE NOT staged ORDER BY id",
    "SELECT * FROM ai_jobs ORDER BY id",
    "SELECT id, next_task_number FROM users",
    "SELECT id, owner_id, actor, ops, effects, created_at FROM mutations ORDER BY id",
    "SELECT * FROM ai_notes ORDER BY folder_id",
];

struct Device {
    st: AppState,
    view: Arc<View>,
    user: String,
    model: Arc<Model>,
    /// Its data directory, deleted with it.
    dir: PathBuf,
    /// How far its clock is ahead of the system's, in milliseconds.
    ahead: Arc<AtomicI64>,
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A model answering each chat with a task titled `title`, held back while the test holds it.
struct Model {
    title: String,
    asked: Mutex<usize>,
    held: Mutex<Option<mpsc::Receiver<()>>>,
}

impl Model {
    /// Holds the next chat back until the sender is used.
    fn hold(&self) -> mpsc::Sender<()> {
        let (tx, rx) = mpsc::channel();
        *self.held.lock().unwrap() = Some(rx);
        tx
    }

    fn asked(&self) -> usize {
        *self.asked.lock().unwrap()
    }
}

impl Api for Model {
    fn chat(&self, _: &Config, _: &Prompt, _: bool, _: Duration) -> Result<String, Failure> {
        *self.asked.lock().unwrap() += 1;
        let held = self.held.lock().unwrap().take();
        if let Some(rx) = held {
            rx.recv().unwrap();
        }
        Ok(json!({ "tasks": [{ "title": self.title }] }).to_string())
    }

    fn models(&self, _: Endpoint) -> Result<Vec<String>, Failure> {
        unreachable!()
    }
}

fn model(device: usize) -> Arc<Model> {
    Arc::new(Model { title: format!("设备 {device}"), asked: Mutex::default(), held: Mutex::default() })
}

fn data_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nexttodo-drive-{}", id()));
    std::fs::create_dir(&dir).unwrap();
    dir
}

/// A database of its own local user.
fn own() -> Db {
    same_user(1).0.remove(0)
}

/// The device on `db`, with its data in `dir`, reaching `store` through a view of its own; not
/// syncing yet.
fn device(db: Db, store: &Arc<Store>, model: Arc<Model>, dir: PathBuf) -> Device {
    let user = db.hold(|c| c.query_row("SELECT id FROM users", [], |r| r.get(0)).unwrap());
    // Where the app keeps them: beside the database, which a backup takes them with.
    let files = Files::new(dir.join(backup::DB_DIR).join(crate::files::DIR));
    let (ahead, system) = (Arc::new(AtomicI64::new(0)), Clock::system());
    let by = ahead.clone();
    let clock = Clock(Arc::new(move || system.now() + by.load(Ordering::SeqCst)));
    let st = AppState { local: Some(dir.clone()), ..AppState::new(db, clock, files).with_ai(Ai::new(None, model.clone())) };
    Device { st, view: Arc::new(View::new(store.clone())), user, model, dir, ahead }
}

/// Settings for a drive, which the tests reach in memory instead.
fn config() -> setup::Config {
    setup::Config::Webdav { url: "https://dav.example.com/dav/".into(), user: "u".into(), password: "p".into(), folder: "NextTodo".into() }
}

impl Device {
    fn db(&self) -> &Db {
        &self.st.db
    }

    /// The drive it syncs through now, if it does.
    fn syncing(&self) -> Option<Arc<Drive>> {
        self.st.drive.drive.try_read().expect("not being turned on or off").clone()
    }

    fn drive(&self) -> Arc<Drive> {
        self.syncing().expect("syncing")
    }

    /// Syncs as `sync_state` says, as the app does when it starts.
    async fn start(&self) {
        Drive::start(&self.st, self.dir.clone(), self.view.clone()).await.unwrap();
    }

    /// The device as the app starts again, with the data a restore it was given has, and syncing as
    /// that says.
    async fn restarted(self, store: &Arc<Store>) -> Device {
        let d = self.reopened(store);
        d.start().await;
        d
    }

    /// The device as the app starts again, with the data a restore it was given has; not syncing.
    fn reopened(mut self, store: &Arc<Store>) -> Device {
        let (dir, model) = (std::mem::take(&mut self.dir), self.model.clone());
        drop(self);
        backup::finish(&dir).unwrap();
        device(Db::open(&dir.join(backup::DB_DIR).join(backup::DB_FILE)).unwrap(), store, model, dir)
    }

    /// Stops syncing in the settings, even with files not here when `force`.
    async fn stop(&self, force: bool) -> Result<StatusCode, ApiError> {
        let query = serde_json::from_value(json!({ "force": force })).unwrap();
        setup::delete(State(self.st.clone()), self.session(), Ok(Query(query))).await
    }

    fn generation(&self) -> String {
        self.db().hold(|c| c.query_row("SELECT generation FROM sync_state", [], |r| r.get(0)).unwrap())
    }

    fn maker(&self) -> Maker {
        Maker { user: self.user.clone(), token_id: String::new(), via: None }
    }

    /// Queues a change of `ops`.
    fn change(&self, ops: serde_json::Value, deadline: Option<Duration>) -> impl Future<Output = Result<Applied, ApiError>> + use<> {
        let m: Mutation = serde_json::from_value(json!({ "id": id(), "ops": ops })).unwrap();
        self.drive().mutate(self.maker(), m, deadline.map(|d| Instant::now() + d))
    }

    /// Makes a change of `ops` as an HTTP request does: through the drive while syncing.
    async fn made(&self, ops: serde_json::Value) -> Result<Applied, ApiError> {
        let m: Mutation = serde_json::from_value(json!({ "id": id(), "ops": ops })).unwrap();
        crate::sync::mutate(State(self.st.clone()), self.session(), Json(m)).await.map(|Json(applied)| applied)
    }

    /// Has the device organize job `job` with the settings it keeps, as the page does.
    async fn run(&self, job: &str) -> Result<StatusCode, ApiError> {
        let config: Config = serde_json::from_value(json!({ "provider": "deepseek", "base_url": "https://api.deepseek.com", "model": "m", "thinking": "off", "api_key": "sk-test" })).unwrap();
        ai::run(State(self.st.clone()), self.session(), Path(job.to_owned()), Json(config)).await
    }

    /// As the page is signed in.
    fn session(&self) -> Auth {
        Auth {
            user_id: self.user.clone(),
            token_id: String::new(),
            kind: TokenKind::Session,
            scope: Scope::Write,
            name: String::new(),
            is_admin: false,
        }
    }

    fn job(&self, job: &str) -> Option<(String, Option<String>)> {
        let rows = dump(self.db(), &format!("SELECT state, error FROM ai_jobs WHERE id = '{job}'"));
        rows.first().map(|r| match (&r[0], &r[1]) {
            (Value::Text(state), Value::Text(error)) => (state.clone(), Some(error.clone())),
            (Value::Text(state), _) => (state.clone(), None),
            _ => unreachable!(),
        })
    }

    fn titles(&self) -> Vec<Vec<Value>> {
        dump(self.db(), "SELECT title FROM tasks ORDER BY title")
    }

    /// Uploads `text` as file `id`, as the page does.
    async fn upload(&self, id: &str, text: &str) -> Result<(), ApiError> {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        let source = Query(crate::attachments::Source { from: None });
        crate::attachments::put_file(
            State(self.st.clone()),
            self.session(),
            Path(id.to_owned()),
            source,
            headers,
            Body::from(text.to_owned()),
        )
        .await
        .map(|_| ())
    }

    /// Has the server copy attachment `from`'s file as file `id`, as the page does copying a folder.
    async fn copy_file(&self, id: &str, from: &str) -> Result<(), ApiError> {
        let source = Query(crate::attachments::Source { from: Some(from.to_owned()) });
        crate::attachments::put_file(State(self.st.clone()), self.session(), Path(id.to_owned()), source, HeaderMap::new(), Body::empty())
            .await
            .map(|_| ())
    }

    /// Calls tool `name` as an agent does, as change `id`.
    async fn tool(&self, name: &str, args: serde_json::Value, id: MutationId) -> Result<Reply, ApiError> {
        let auth = Auth {
            user_id: self.user.clone(),
            token_id: String::new(),
            kind: TokenKind::Api,
            scope: Scope::Write,
            name: "agent".into(),
            is_admin: false,
        };
        tools::call(&self.st, auth, name, args, Some(id)).await
    }

    fn applied(&self) -> i64 {
        self.db().hold(|c| c.query_row("SELECT applied FROM sync_state", [], |r| r.get(0)).unwrap())
    }

    fn count(&self, sql: &str) -> i64 {
        self.db().hold(|c| c.query_row(sql, [], |r| r.get(0)).unwrap())
    }
}

/// `n` devices syncing through generation `g` of a drive, from the same data.
async fn devices(n: usize) -> (Arc<Store>, Vec<Device>) {
    let store = Arc::new(Store::default());
    View::new(store.clone()).create(setup::POINTER, &setup::pointer("g")).unwrap();
    let mut devices = Vec::new();
    for (i, db) in same_user(n).0.into_iter().enumerate() {
        db.hold(|c| c.execute("INSERT INTO sync_state (id, generation, applied) VALUES (1, 'g', 0)", []).unwrap());
        let d = device(db, &store, model(i + 1), data_dir());
        d.start().await;
        devices.push(d);
    }
    (store, devices)
}

/// The commits in the drive.
fn log(store: &Store) -> Vec<String> {
    store.paths().into_iter().filter(|p| p.contains("/log/")).collect()
}

/// Waits until every device has read every commit in the drive.
async fn caught_up(store: &Store, devices: &[Device]) {
    let commits = i64::try_from(log(store).len()).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while devices.iter().any(|d| d.applied() < commits) {
        assert!(Instant::now() < until, "devices did not catch up");
        for d in devices {
            d.drive().pull();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Waits until `done` holds.
async fn until(done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "timed out");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Waits for a write to be held at its gate.
async fn arrived(arrived: std::sync::mpsc::Receiver<()>) {
    tokio::task::spawn_blocking(move || arrived.recv().unwrap()).await.unwrap();
}

fn assert_same(devices: &[Device]) {
    for sql in SHARED {
        let first = dump(devices[0].db(), sql);
        for d in &devices[1..] {
            assert_eq!(first, dump(d.db(), sql), "{sql}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_made_at_once_on_several_devices_all_go_in_and_every_device_ends_the_same() {
    let (store, devices) = devices(3).await;
    let waiting: Vec<_> = (0..5)
        .flat_map(|i| {
            devices.iter().map(move |d| d.change(json!([{ "op": "task.create", "id": id(), "title": format!("第 {i} 项") }]), None))
        })
        .collect();
    for w in waiting {
        w.await.unwrap();
    }
    caught_up(&store, &devices).await;
    assert_same(&devices);
    let numbers: Vec<Vec<Value>> = dump(devices[0].db(), "SELECT number FROM tasks ORDER BY number");
    assert_eq!(numbers, (1..=15).map(|n| vec![Value::Integer(n)]).collect::<Vec<_>>(), "every task has a number of its own");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_completion_date_edit_and_its_undo_sync_between_devices() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let task = id();
    a.made(json!([
        { "op": "task.create", "id": task, "title": "组会", "due": { "date": "2020-11-20", "repeat": { "every": 2, "unit": "day", "from": "done" } } },
        { "op": "task.update", "id": task, "done": true, "today": "2020-11-20" },
    ])).await.unwrap();
    caught_up(&store, &devices).await;
    let task_dates = "SELECT due_date, last_completed FROM tasks";
    let record_dates = "SELECT due_date, done_on FROM completions";
    let old = (dump(b.db(), task_dates), dump(b.db(), record_dates));

    let edit = id();
    let m = serde_json::from_value(json!({ "id": edit, "today": "2020-11-25", "ops": [
        { "op": "completion.update", "task_id": task, "due": "2020-11-20", "again": 0, "done": "2020-11-19" }
    ] }))
    .unwrap();
    a.drive().mutate(a.maker(), m, None).await.unwrap();
    let bytes = b.view.get("g/log/2.json").unwrap().unwrap();
    let commit = super::parse(&bytes).unwrap();
    assert_eq!(commit.format, 11);
    caught_up(&store, &devices).await;
    assert_eq!(dump(b.db(), task_dates), vec![vec![Value::Text("2020-11-21".into()), Value::Text("2020-11-19".into())]]);
    assert_eq!(dump(b.db(), record_dates), vec![vec![Value::Text("2020-11-20".into()), Value::Text("2020-11-19".into())]]);

    b.made(json!([{ "op": "undo", "mutation": edit }])).await.unwrap();
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!((dump(a.db(), task_dates), dump(a.db(), record_dates)), old);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_another_device_made_impossible_meanwhile_is_turned_down() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let (t1, t2) = (id(), id());
    a.made(json!([{ "op": "task.create", "id": t1, "title": "一" }, { "op": "task.create", "id": t2, "title": "二" }])).await.unwrap();
    caught_up(&store, &devices).await;
    // `b` tries its change on the data it has, and is about to write it as `a` writes its own.
    let gate = b.view.gate_next_write();
    let turned_down = b.change(json!([{ "op": "dep.add", "before": t2, "after": t1 }]), None);
    arrived(gate.arrived).await;
    a.made(json!([{ "op": "dep.add", "before": t1, "after": t2 }])).await.unwrap();
    gate.open.send(()).unwrap();
    assert_eq!(turned_down.await.unwrap_err().code(), "cycle");
    caught_up(&store, &devices).await;
    assert_same(&devices);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_written_without_an_answer_counts_once() {
    let (store, devices) = devices(2).await;
    devices[0].view.fail_writes([Fault::AnswerLost]);
    devices[0].made(json!([{ "op": "task.create", "id": id(), "title": "一" }])).await.unwrap();
    caught_up(&store, &devices).await;
    assert_eq!(log(&store), ["g/log/1.json"]);
    assert_eq!(devices[1].count("SELECT count(*) FROM tasks"), 1);
    assert_same(&devices);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_lost_on_the_way_is_sent_again_and_what_follows_goes_on() {
    let (store, devices) = devices(2).await;
    devices[0].view.fail_writes([Fault::RequestLost]);
    devices[0].made(json!([{ "op": "task.create", "id": id(), "title": "一" }])).await.unwrap();
    devices[0].made(json!([{ "op": "task.create", "id": id(), "title": "二" }])).await.unwrap();
    caught_up(&store, &devices).await;
    assert_eq!(log(&store), ["g/log/1.json", "g/log/2.json"]);
    assert_same(&devices);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_far_behind_reads_several_commits_at_once_and_one_not_read_or_not_there_yet_among_them_again() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    b.view.set_offline(true);
    for i in 0..40 {
        a.made(json!([{ "op": "task.create", "id": id(), "title": format!("第 {i} 项") }])).await.unwrap();
    }
    // 2 is read with 3 as if before it was written, then alone; 6 is lost with 5, 7 and 8, then read alone.
    b.view.read_early("g/log/2.json");
    b.view.lose_read("g/log/6.json");
    b.view.set_offline(false);
    assert!(matches!(b.drive().sync(Duration::from_secs(5)).await, Synced::Done));
    assert_eq!(b.applied(), 40);
    assert_same(&devices);
    assert_eq!(log(&store).len(), 40);
}

#[tokio::test(flavor = "multi_thread")]
async fn syncing_now_reads_in_what_another_device_just_wrote_and_says_at_once_when_held_up() {
    let (_, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    // Looked at the drive as it started: it would look again in half a minute.
    until(|| b.drive().status().synced_at.is_some()).await;
    a.made(json!([{ "op": "task.create", "id": id(), "title": "一" }])).await.unwrap();
    let synced = b.tool("sync", json!({}), MutationId(Uuid::now_v7())).await.unwrap();
    assert_eq!((synced.output.text.as_str(), &synced.output.data["done"]), ("已同步。", &json!(true)));
    assert_eq!(b.titles(), [[Value::Text("一".into())]]);
    b.view.set_offline(true);
    let asked = Instant::now();
    let Err(e) = b.tool("sync", json!({}), MutationId(Uuid::now_v7())).await else { panic!("said it synced") };
    assert_eq!((e.code(), e.to_string().as_str()), ("unavailable", "同步受阻：offline"));
    assert!(asked.elapsed() < Duration::from_secs(5), "not waiting out the 15 seconds");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_not_sent_by_its_deadline_is_not_made() {
    let (store, devices) = devices(1).await;
    let d = &devices[0];
    d.view.set_offline(true);
    let e = d.change(json!([{ "op": "task.create", "id": id(), "title": "一" }]), Some(Duration::from_millis(300))).await.unwrap_err();
    assert_eq!((e.code(), e.to_string().as_str()), ("unavailable", NOT_MADE));
    d.view.set_offline(false);
    d.made(json!([{ "op": "task.create", "id": id(), "title": "二" }])).await.unwrap();
    caught_up(&store, &devices).await;
    assert_eq!(d.count("SELECT count(*) FROM tasks"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_that_may_be_in_the_drive_is_said_so_and_sent_again_is_answered_as_made_once() {
    let (store, devices) = devices(1).await;
    let d = &devices[0];
    // The write is lost as the connection drops: what became of it can't be found out for now.
    let gate = d.view.gate_next_write();
    d.view.fail_writes([Fault::RequestLost]);
    let m: Mutation = serde_json::from_value(json!({ "id": id(), "ops": [{ "op": "task.create", "id": id(), "title": "一" }] })).unwrap();
    let waiting = d.drive().mutate(d.maker(), m.clone(), Some(Instant::now() + Duration::from_millis(300)));
    arrived(gate.arrived).await;
    d.view.set_offline(true);
    gate.open.send(()).unwrap();
    let e = waiting.await.unwrap_err();
    assert_eq!((e.code(), e.describe()), ("outcome_unknown", format!("{}（request_id：{}）", e, m.id)));
    d.view.set_offline(false);
    // Answered once the commit sent is settled: it went in.
    let again = d.drive().mutate(d.maker(), m, None).await.unwrap();
    assert_eq!(again.seq, 1);
    caught_up(&store, &devices).await;
    assert_eq!(d.count("SELECT count(*) FROM tasks"), 1);
    assert_eq!(log(&store), ["g/log/1.json"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_are_answered_by_their_deadline_even_as_the_drive_hangs() {
    let (store, devices) = devices(1).await;
    let d = &devices[0];
    let gate = d.view.gate_next_write();
    let sent = d.change(task("发出的"), Some(Duration::from_millis(300)));
    arrived(gate.arrived).await;
    let queued = d.change(task("排着的"), Some(Duration::from_millis(300)));
    for (waiting, said) in [(sent, "outcome_unknown"), (queued, "unavailable")] {
        let e = tokio::time::timeout(Duration::from_secs(5), waiting).await.expect("answered by the deadline").unwrap_err();
        assert_eq!(e.code(), said);
    }
    gate.open.send(()).unwrap();
    until(|| log(&store).len() == 1).await;
    caught_up(&store, &devices).await;
    assert_eq!(d.titles(), [[Value::Text("发出的".into())]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_without_an_answer_is_not_sent_again_into_a_folder_made_anew() {
    let (store, devices) = devices(1).await;
    let d = &devices[0];
    let gate = d.view.gate_next_write();
    d.view.fail_writes([Fault::RequestLost]);
    let waiting = d.change(task("一"), None);
    arrived(gate.arrived).await;
    // As the commit is on its way, and lost, the folder is deleted and made anew by another device.
    let other = View::new(store.clone());
    other.delete(setup::POINTER).unwrap();
    other.create(setup::POINTER, &setup::pointer("h")).unwrap();
    gate.open.send(()).unwrap();
    assert_eq!(waiting.await.unwrap_err().code(), "internal");
    assert_eq!(d.st.drive.stopped().unwrap(), Trouble::Replaced.message());
    assert!(log(&store).is_empty(), "{:?}", store.paths());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_from_the_page_waits_while_the_drive_is_unreachable() {
    let (store, devices) = devices(2).await;
    devices[0].view.set_offline(true);
    let waiting = devices[0].change(json!([{ "op": "task.create", "id": id(), "title": "一" }]), None);
    while devices[0].view.refused() == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    devices[0].view.set_offline(false);
    devices[0].drive().pull();
    waiting.await.unwrap();
    caught_up(&store, &devices).await;
    assert_same(&devices);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_sent_again_is_made_once() {
    let (store, devices) = devices(1).await;
    let d = &devices[0];
    let m: Mutation = serde_json::from_value(json!({ "id": id(), "ops": [{ "op": "task.create", "id": id(), "title": "一" }] })).unwrap();
    let (first, again) = (d.drive().mutate(d.maker(), m.clone(), None), d.drive().mutate(d.maker(), m, None));
    assert_eq!(first.await.unwrap().seq, again.await.unwrap().seq);
    caught_up(&store, &devices).await;
    assert_eq!(d.count("SELECT count(*) FROM tasks"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_is_built_again_on_what_another_device_made_meanwhile_and_a_retry_gets_its_answer() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    // `b` builds its tool on the data it has, and is about to write it as `a` makes the folder it names.
    let gate = b.view.gate_next_write();
    let (call, add) = (MutationId(Uuid::now_v7()), json!({ "title": "写周报", "folder": "工作" }));
    let (answer, ()) = tokio::join!(b.tool("add", add.clone(), call), async {
        arrived(gate.arrived).await;
        a.made(json!([{ "op": "folder.create", "id": id(), "name": "工作" }])).await.unwrap();
        gate.open.send(()).unwrap();
    });
    let answer = answer.unwrap();
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!(b.count("SELECT count(*) FROM folders"), 1, "the task went into the folder `a` made");
    assert_eq!(b.count("SELECT count(*) FROM tasks t JOIN folders f ON f.id = t.folder_id"), 1);
    let again = b.tool("add", add, call).await.unwrap();
    assert_eq!((again.output.text, again.output.data), (answer.output.text, answer.output.data));
    assert_eq!(b.count("SELECT count(*) FROM tasks"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_preview_changes_nothing_anywhere() {
    let (store, devices) = devices(1).await;
    let d = &devices[0];
    let plan = json!({ "dry_run": true, "plan": { "tasks": [{ "title": "一" }] } });
    let preview = d.tool("apply", plan, MutationId(Uuid::now_v7())).await.unwrap();
    assert!(preview.output.text.starts_with("预览"), "{}", preview.output.text);
    assert!(log(&store).is_empty());
    assert_eq!(d.count("SELECT count(*) FROM tasks"), 0);
}

fn job_create(job: &str) -> serde_json::Value {
    json!([{ "op": "ai_job.create", "id": job, "text": "整理一下", "local_time": "2026-11-17 10:00 星期二" }])
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_device_that_writes_the_start_organizes_a_job() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let job = id();
    a.made(job_create(&job)).await.unwrap();
    caught_up(&store, &devices).await;
    // Both start it on the data they have; `b` is about to write its start as `a` writes its own.
    let gate = b.view.gate_next_write();
    let (from_a, from_b) = tokio::join!(a.run(&job), async {
        let started = b.run(&job);
        let opened = async {
            arrived(gate.arrived).await;
            until(|| !a.job(&job).is_some_and(|(state, _)| state == "waiting")).await;
            gate.open.send(()).unwrap();
        };
        tokio::join!(started, opened).0
    });
    assert_eq!(from_a.unwrap(), StatusCode::ACCEPTED);
    assert_eq!(from_b.unwrap_err().code(), "conflict");
    until(|| a.job(&job).is_none()).await;
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!((a.model.asked(), b.model.asked()), (1, 0));
    assert_eq!(b.titles(), [[Value::Text("设备 1".into())]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_answer_of_an_earlier_round_does_not_count_in_the_round_since() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let job = id();
    a.made(job_create(&job)).await.unwrap();
    let late = a.model.hold();
    assert_eq!(a.run(&job).await.unwrap(), StatusCode::ACCEPTED);
    caught_up(&store, &devices).await;
    // Deleted and that undone, the job is tried again on `b` while `a` still waits for its model.
    let delete = b.made(json!([{ "op": "ai_job.delete", "id": job }])).await.unwrap();
    let undo = serde_json::to_value(
        b.db().hold(|c| c.query_row("SELECT id FROM mutations WHERE seq = ?1", [delete.seq], |r| r.get::<_, String>(0))).unwrap(),
    )
    .unwrap();
    b.made(json!([{ "op": "undo", "mutation": undo }])).await.unwrap();
    b.made(json!([{ "op": "ai_job.retry", "id": job }])).await.unwrap();
    let held = b.model.hold();
    assert_eq!(b.run(&job).await.unwrap(), StatusCode::ACCEPTED);
    until(|| b.model.asked() == 1).await;
    late.send(()).unwrap();
    until(|| {
        a.model.asked() == 1 && a.db().hold(|c| c.query_row("SELECT count(*) FROM sync_runs", [], |r| r.get::<_, i64>(0)).unwrap()) == 0
    })
    .await;
    caught_up(&store, &devices).await;
    assert_eq!(b.job(&job).unwrap().0, "running", "the late answer was thrown away");
    held.send(()).unwrap();
    until(|| b.job(&job).is_none()).await;
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!(a.titles(), [[Value::Text("设备 2".into())]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_round_started_but_never_read_in_before_the_app_stopped_is_failed_on_restart() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let job = id();
    a.made(job_create(&job)).await.unwrap();
    caught_up(&store, &devices).await;
    // `a` wrote down the round and its start went into the drive, but `a` stopped before reading it.
    let (run, job_id) = (MutationId(Uuid::now_v7()), serde_json::from_value::<AiJobId>(json!(job)).unwrap());
    a.db().hold(|c| c.execute("INSERT INTO sync_runs (run, job_id) VALUES (?1, ?2)", rusqlite::params![run, job_id]).unwrap());
    let start: StepFn = Box::new(move |c, actor, now| {
        let running = |j| AiJob { state: JobState::Running, ..j };
        Ok(mutate::job_change(c, actor, job_id, JobState::Waiting, running, now)?.map(|(_, done)| (Vec::new(), done)))
    });
    let maker = Maker { user: b.user.clone(), token_id: String::new(), via: Some(ai::ACTOR.into()) };
    assert!(b.drive().step(maker, run, start).await.unwrap());
    assert_eq!(a.job(&job).unwrap().0, "waiting");
    ai::interrupted(&a.st).await.unwrap();
    until(|| a.job(&job).is_some_and(|(state, _)| state == "failed")).await;
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!(b.job(&job), Some(("failed".into(), Some("服务器重启，整理中断".into()))));
    until(|| a.count("SELECT count(*) FROM sync_runs") == 0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_being_organized_as_syncing_is_turned_on_fails_and_its_late_answer_never_counts() {
    let store = Arc::new(Store::default());
    let a = device(own(), &store, model(1), data_dir());
    let job = id();
    a.made(job_create(&job)).await.unwrap();
    let late = a.model.hold();
    assert_eq!(a.run(&job).await.unwrap(), StatusCode::ACCEPTED);
    until(|| a.model.asked() == 1).await;
    setup::turn_on(&a.st, &a.dir, config(), a.view.clone(), false).await.unwrap();
    until(|| !a.drive().status().enabling).await;
    assert_eq!(a.job(&job), Some(("failed".into(), Some("开启同步，整理中断".into()))));
    // Tried again on a device joining, as `a` still waits for its model.
    let b = device(own(), &store, model(2), data_dir());
    setup::turn_on(&b.st, &b.dir, config(), b.view.clone(), true).await.unwrap();
    let devices = [a, b.restarted(&store).await];
    let [a, b] = &devices;
    b.made(json!([{ "op": "ai_job.retry", "id": job }])).await.unwrap();
    let held = b.model.hold();
    assert_eq!(b.run(&job).await.unwrap(), StatusCode::ACCEPTED);
    until(|| b.model.asked() == 1).await;
    caught_up(&store, &devices).await;
    // `a` has the job in `b`'s round, and stops syncing before its own answer comes.
    assert_eq!(a.stop(false).await.unwrap(), StatusCode::NO_CONTENT);
    late.send(()).unwrap();
    // `a`'s round is over once it can try the job again, which is no longer waiting.
    let by = Instant::now() + Duration::from_secs(10);
    while a.run(&job).await.unwrap_err().to_string() != "这条请求不在等待整理" {
        assert!(Instant::now() < by, "the round on `a` did not end");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(a.titles().is_empty(), "{:?}", a.titles());
    held.send(()).unwrap();
    until(|| b.job(&job).is_none()).await;
    assert_eq!(b.titles(), [[Value::Text("设备 2".into())]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_note_for_ai_organizing_goes_to_every_device() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let folder = id();
    a.made(json!([{ "op": "folder.create", "id": folder, "name": "工作" }])).await.unwrap();
    let note = |folder: &str| Json(serde_json::from_value::<ai::Note>(json!({ "folder_id": folder, "text": "周报周五交" })).unwrap());
    assert_eq!(ai::put_note(State(a.st.clone()), a.session(), note(&folder)).await.unwrap(), StatusCode::NO_CONTENT);
    assert_eq!(ai::put_note(State(b.st.clone()), b.session(), note(&id())).await.unwrap_err().code(), "not_found");
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!(b.count("SELECT count(*) FROM ai_notes"), 1);
}

fn file(id: &str) -> AttachmentId {
    serde_json::from_value(json!(id)).unwrap()
}

/// The size of a file of `text`.
fn size(text: &str) -> FileSize {
    serde_json::from_value(json!(text.len())).unwrap()
}

/// Makes a task and attaches file `file`, uploaded on `d`, to it.
async fn attach(d: &Device, file: &str) {
    let task = id();
    d.upload(file, "你好").await.unwrap();
    d.made(json!([
        { "op": "task.create", "id": task, "title": "看附件" },
        { "op": "attachment.add", "id": file, "name": "说明.txt", "type": "text/plain", "size": "你好".len(), "task_id": task },
    ]))
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attachment_added_on_one_device_is_read_on_another_from_the_drive() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let f = id();
    attach(a, &f).await;
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert!(store.paths().contains(&format!("g/files/{f}")));
    assert_eq!(b.st.files.here(file(&f)).unwrap(), None);
    assert_eq!(b.st.files.read(file(&f), size("你好")).unwrap(), "你好".as_bytes());
    assert!(b.st.files.here(file(&f)).unwrap().is_some(), "kept here once fetched");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_copied_where_its_files_are_not_yet_copies_them_from_the_drive() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let (folder, task, f) = (id(), id(), id());
    a.upload(&f, "你好").await.unwrap();
    a.made(json!([
        { "op": "folder.create", "id": folder, "name": "合同" },
        { "op": "task.create", "id": task, "title": "看附件", "folder_id": folder },
        { "op": "attachment.add", "id": f, "name": "说明.txt", "type": "text/plain", "size": "你好".len(), "task_id": task },
    ]))
    .await
    .unwrap();
    let g = id();
    attach(a, &g).await;
    caught_up(&store, &devices).await;
    assert_eq!((b.st.files.here(file(&f)).unwrap(), b.st.files.here(file(&g)).unwrap()), (None, None));

    // By an agent, and by the page, each fetching a file not here.
    b.tool("copy", json!({ "folder": "/合同" }), MutationId(Uuid::now_v7())).await.unwrap();
    let (copy, other) = (id(), id());
    b.copy_file(&copy, &g).await.unwrap();
    b.made(json!([
        { "op": "task.create", "id": other, "title": "再看" },
        { "op": "attachment.add", "id": copy, "name": "说明.txt", "type": "text/plain", "size": "你好".len(), "task_id": other },
    ]))
    .await
    .unwrap();
    caught_up(&store, &devices).await;
    assert_same(&devices);
    let ids = dump(a.db(), "SELECT id FROM attachments ORDER BY id");
    assert_eq!(ids.len(), 4);
    for row in ids {
        let Value::Text(id) = &row[0] else { unreachable!() };
        assert_eq!(a.st.files.read(file(id), size("你好")).unwrap(), "你好".as_bytes(), "{id} in the drive");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_in_the_drive_not_of_the_size_recorded_is_not_kept() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let f = id();
    attach(a, &f).await;
    caught_up(&store, &devices).await;
    let path = format!("g/files/{f}");
    a.view.delete(&path).unwrap();
    a.view.create(&path, "你".as_bytes()).unwrap();
    assert!(b.st.files.read(file(&f), size("你好")).is_err());
    assert_eq!(b.st.files.here(file(&f)).unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attachment_whose_id_another_file_has_in_the_drive_is_turned_down() {
    let (store, devices) = devices(1).await;
    let a = &devices[0];
    let (f, task) = (id(), id());
    a.view.create(&format!("g/files/{f}"), "再见".as_bytes()).unwrap();
    a.upload(&f, "你好").await.unwrap();
    let added = a
        .made(json!([
            { "op": "task.create", "id": task, "title": "看附件" },
            { "op": "attachment.add", "id": f, "name": "说明.txt", "type": "text/plain", "size": "你好".len(), "task_id": task },
        ]))
        .await;
    assert_eq!(added.unwrap_err().code(), "conflict");
    // The same again, as one device does when it was not told: there already, it goes in.
    let g = id();
    a.view.create(&format!("g/files/{g}"), "你好".as_bytes()).unwrap();
    attach(a, &g).await;
    assert_eq!(log(&store).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn files_cleared_through_the_drive_are_gone_everywhere_and_their_ids_not_used_again() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let (attached, staged) = (id(), id());
    attach(a, &attached).await;
    // Uploaded on `b` and never attached: `b`'s alone, until it is cleared.
    b.upload(&staged, "草稿").await.unwrap();
    let delete = a.made(json!([{ "op": "attachment.delete", "id": attached }])).await.unwrap();
    caught_up(&store, &devices).await;
    let later = a.st.clock.now() + 1000;
    a.drive().purge(later).await.unwrap();
    b.drive().purge(later).await.unwrap();
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!(a.count("SELECT count(*) FROM files WHERE purged"), 2);
    for f in [&attached, &staged] {
        assert!(!store.paths().contains(&format!("g/files/{f}")));
        for d in &devices {
            assert_eq!(d.st.files.here(file(f)).unwrap(), None);
        }
    }
    assert_eq!(a.upload(&attached, "你好").await.unwrap_err().code(), "conflict");
    let undo = a.db().hold(|c| c.query_row("SELECT id FROM mutations WHERE seq = ?1", [delete.seq], |r| r.get::<_, String>(0)).unwrap());
    assert_eq!(a.made(json!([{ "op": "undo", "mutation": undo }])).await.unwrap_err().code(), "not_found");
}

fn task(title: &str) -> serde_json::Value {
    json!([{ "op": "task.create", "id": id(), "title": title }])
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_turning_syncing_on_puts_its_data_in_the_drive_and_one_joining_takes_that_for_its_own() {
    let store = Arc::new(Store::default());
    let a = device(own(), &store, model(1), data_dir());
    let f = id();
    attach(&a, &f).await;
    setup::turn_on(&a.st, &a.dir, config(), a.view.clone(), false).await.unwrap();
    // Made as the first generation is made: it waits for it, and goes in after.
    a.made(task("开启时改的")).await.unwrap();
    let g = a.generation();
    assert_eq!(
        store.paths(),
        [format!("{g}/files/{f}"), format!("{g}/log/1.json"), format!("{g}/snapshots/0.db.gz"), setup::POINTER.into()]
    );
    assert!(!a.drive().status().enabling);

    let b = device(own(), &store, model(2), data_dir());
    b.made(task("这台设备原来的")).await.unwrap();
    assert_eq!(setup::turn_on(&b.st, &b.dir, config(), b.view.clone(), false).await.unwrap_err().code(), "has_data");
    setup::turn_on(&b.st, &b.dir, config(), b.view.clone(), true).await.unwrap();
    let devices = [a, b.restarted(&store).await];
    let [a, b] = &devices;
    caught_up(&store, &devices).await;
    assert_same(&devices);
    assert_eq!(b.titles(), [[Value::Text("开启时改的".into())], [Value::Text("看附件".into())]]);
    assert_ne!(dump(a.db(), "SELECT value FROM epoch"), dump(b.db(), "SELECT value FROM epoch"));
    assert_eq!(b.st.files.read(file(&f), size("你好")).unwrap(), "你好".as_bytes());
    b.made(task("加入后改的")).await.unwrap();
    caught_up(&store, &devices).await;
    assert_same(&devices);
}

#[tokio::test(flavor = "multi_thread")]
async fn of_two_devices_turning_syncing_on_in_one_empty_folder_at_once_one_does_and_the_other_says_so() {
    let store = Arc::new(Store::default());
    let (a, b) = (device(own(), &store, model(1), data_dir()), device(own(), &store, model(2), data_dir()));
    // Both found the folder empty.
    let (on_a, on_b) =
        tokio::join!(Drive::enable(&a.st, a.dir.clone(), a.view.clone()), Drive::enable(&b.st, b.dir.clone(), b.view.clone()));
    on_a.unwrap();
    on_b.unwrap();
    until(|| a.st.drive.stopped().is_some() || b.st.drive.stopped().is_some()).await;
    let (on, off) = if a.st.drive.stopped().is_some() { (&b, &a) } else { (&a, &b) };
    assert_eq!(off.st.drive.stopped().unwrap(), Trouble::Taken.message());
    assert!(off.syncing().is_none());
    assert_eq!(off.count("SELECT count(*) FROM sync_state"), 0);
    until(|| !on.drive().status().enabling).await;
    let g = on.generation();
    assert!(store.paths().iter().all(|p| p == setup::POINTER || p.starts_with(&g)), "{:?}", store.paths());
    off.made(task("只在这台设备上")).await.unwrap();
    on.made(task("写进网盘")).await.unwrap();
    assert_eq!(log(&store), [format!("{g}/log/1.json")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_stops_syncing_before_it_writes_into_a_folder_made_anew() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    a.made(task("一")).await.unwrap();
    caught_up(&store, &devices).await;
    // Stopping leaves the drive as it is.
    assert_eq!(b.stop(false).await.unwrap(), StatusCode::NO_CONTENT);
    assert!(b.syncing().is_none());
    b.made(task("只在 b 上")).await.unwrap();
    assert_eq!(log(&store), ["g/log/1.json"]);
    // The folder deleted, and syncing turned on in it again with `b`'s data, as `a` runs.
    b.view.delete("g").unwrap();
    b.view.delete(setup::POINTER).unwrap();
    setup::turn_on(&b.st, &b.dir, config(), b.view.clone(), false).await.unwrap();
    // What waited on `a` is sent again by the page, and made here alone.
    assert_eq!(a.made(task("二")).await.unwrap_err().code(), "internal");
    assert_eq!(a.st.drive.stopped().unwrap(), Trouble::Replaced.message());
    assert!(a.syncing().is_none());
    assert!(log(&store).is_empty(), "{:?}", store.paths());
    a.made(task("二")).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_ends_the_sync_task_at_once_and_it_writes_nothing_here_after() {
    let (store, devices) = devices(1).await;
    let d = &devices[0];
    let gate = d.view.gate_next_write();
    let waiting = d.change(task("停止前改的"), None);
    arrived(gate.arrived).await;
    assert_eq!(d.stop(false).await.unwrap(), StatusCode::NO_CONTENT);
    // Sent again by the page, and made directly.
    let e = tokio::time::timeout(Duration::from_secs(5), waiting).await.expect("answered as syncing stops").unwrap_err();
    assert_eq!(e.code(), "internal");
    d.made(task("停止后改的")).await.unwrap();
    // The write the task left goes on, and its commit is in the drive for the other devices.
    gate.open.send(()).unwrap();
    until(|| log(&store).len() == 1).await;
    assert_eq!(d.titles(), [[Value::Text("停止后改的".into())]]);
    assert_eq!(d.count("SELECT count(*) FROM sync_state"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_downloads_the_files_still_of_use_first_or_asks_before_going_on_without() {
    let (store, devices) = devices(3).await;
    let (a, b, c) = (&devices[0], &devices[1], &devices[2]);
    let (f, deleted) = (id(), id());
    attach(a, &f).await;
    // Deleted, which undoing brings back.
    attach(a, &deleted).await;
    a.made(json!([{ "op": "attachment.delete", "id": deleted }])).await.unwrap();
    caught_up(&store, &devices).await;
    for d in [b, c] {
        d.view.set_offline(true);
        assert_eq!(d.stop(false).await.unwrap_err().code(), "files_not_here");
        assert!(d.syncing().is_some());
    }
    b.view.set_offline(false);
    assert_eq!(b.stop(false).await.unwrap(), StatusCode::NO_CONTENT);
    for f in [&f, &deleted] {
        assert_eq!(b.st.files.here(file(f)).unwrap().as_deref(), Some("你好".as_bytes()));
    }
    assert_eq!(c.stop(true).await.unwrap(), StatusCode::NO_CONTENT);
    assert!(c.syncing().is_none());
    assert_eq!(c.st.files.here(file(&f)).unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_downloads_too_the_files_of_attachments_read_in_as_it_stops() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let job = id();
    b.made(job_create(&job)).await.unwrap();
    let held = b.model.hold();
    assert_eq!(b.run(&job).await.unwrap(), StatusCode::ACCEPTED);
    until(|| b.model.asked() == 1).await;
    caught_up(&store, &devices).await;
    // As `b` stops, failing the job it organizes, `a` adds an attachment, which `b` reads in first.
    let (f, gate) = (id(), b.view.gate_next_write());
    let (stopped, ()) = tokio::join!(b.stop(false), async {
        arrived(gate.arrived).await;
        attach(a, &f).await;
        gate.open.send(()).unwrap();
    });
    assert_eq!(stopped.unwrap(), StatusCode::NO_CONTENT);
    assert_eq!(b.st.files.read(file(&f), size("你好")).unwrap(), "你好".as_bytes());
    assert_eq!(b.job(&job).unwrap().0, "failed");
    held.send(()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_taken_while_syncing_restores_to_data_that_can_be_synced_anew() {
    let (_, mut devices) = devices(1).await;
    let d = devices.remove(0);
    d.made(task("备份前改的")).await.unwrap();
    let backup = std::env::temp_dir().join(format!("nexttodo-backup-{}.db", id()));
    d.db().hold(|c| crate::db::copy(c, &backup)).unwrap();
    d.stop(false).await.unwrap();
    backup::prepare(&d.dir, &backup).unwrap();
    backup::commit(&d.dir).unwrap();
    std::fs::remove_file(&backup).unwrap();
    // On an empty folder, for every device to use the backup.
    let store = Arc::new(Store::default());
    let d = d.reopened(&store);
    setup::turn_on(&d.st, &d.dir, config(), d.view.clone(), false).await.unwrap();
    d.made(task("恢复后改的")).await.unwrap();
    assert_eq!(log(&store), [format!("{}/log/1.json", d.generation())]);
    assert_eq!(d.titles(), [[Value::Text("备份前改的".into())], [Value::Text("恢复后改的".into())]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_taken_while_syncing_has_the_files_only_in_the_drive_and_those_undoing_brings_back() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let (f, deleted) = (id(), id());
    attach(a, &f).await;
    attach(a, &deleted).await;
    a.made(json!([{ "op": "attachment.delete", "id": deleted }])).await.unwrap();
    caught_up(&store, &devices).await;
    let db = b.dir.join(backup::DB_DIR);
    std::fs::create_dir_all(&db).unwrap();
    b.db().hold(|c| crate::db::copy(c, &db.join(backup::DB_FILE))).unwrap();
    let to = b.dir.with_extension("tar");
    backup::write(&b.dir, &b.st.files, &to).unwrap();
    let mut tar = tar::Archive::new(std::fs::File::open(&to).unwrap());
    let names: Vec<String> = tar.entries().unwrap().map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned()).collect();
    std::fs::remove_file(&to).unwrap();
    for f in [&f, &deleted] {
        assert!(names.contains(&format!("files/{f}")), "{names:?}");
    }
}

/// Has `d` make `n` changes, one after another.
async fn many(d: &Device, n: usize) {
    for i in 0..n {
        d.made(task(&format!("第 {i} 项"))).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshots_are_kept_as_commits_go_in_and_a_device_far_behind_reads_the_latest_in_their_place() {
    let (store, devices) = devices(2).await;
    let (a, b) = (&devices[0], &devices[1]);
    let mine = b.made(task("b 离开前改的")).await.unwrap();
    caught_up(&store, &devices).await;
    let mine_id: String = b.db().hold(|c| c.query_row("SELECT id FROM mutations WHERE seq = ?1", [mine.seq], |r| r.get(0)).unwrap());
    // `b` is away for days, as `a` goes on for months.
    b.view.set_offline(true);
    b.ahead.store(2 * DAY_MS, Ordering::SeqCst);
    let has = |path: &str| store.paths().contains(&path.to_owned());
    many(a, 199).await;
    until(|| has("g/snapshots/200.db.gz")).await;
    many(a, 200).await;
    until(|| has("g/snapshots/400.db.gz")).await;
    // Left over from a day and more ago, but for the generation just begun.
    let (old, later) = (
        id(),
        Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, u64::try_from((a.st.clock.now() + 91 * DAY_MS) / 1000).unwrap(), 0)),
    );
    for path in
        [format!("tmp/{}", id()), format!("{old}/snapshots/0.db.gz"), format!("{old}/files/{}", id()), format!("{later}/snapshots/0.db.gz")]
    {
        a.view.create(&path, b"x").unwrap();
    }
    a.ahead.store(91 * DAY_MS, Ordering::SeqCst);
    many(a, 200).await;
    until(|| !store.paths().iter().any(|p| p.starts_with(&old))).await;
    let paths = store.paths();
    assert_eq!(
        paths.iter().filter(|p| p.starts_with("g/snapshots/")).collect::<Vec<_>>(),
        ["g/snapshots/400.db.gz", "g/snapshots/600.db.gz"]
    );
    assert_eq!(log(&store).len(), 400);
    assert!(!has("g/log/200.json") && has("g/log/201.json"));
    assert!(!paths.iter().any(|p| p.starts_with("tmp/")), "{paths:?}");
    assert!(has(&format!("{later}/snapshots/0.db.gz")));

    b.view.set_offline(false);
    b.drive().pull();
    until(|| b.applied() == 600).await;
    assert_same(&devices);
    let kept: i64 = b.db().hold(|c| c.query_row("SELECT seq FROM mutations WHERE id = ?1", [&mine_id], |r| r.get(0)).unwrap());
    assert_eq!(kept, mine.seq, "a change here keeps its number");
    assert_eq!(b.count("SELECT pruned_seq FROM users"), b.count("SELECT max(seq) FROM mutations"), "the pages download everything again");
    b.made(task("b 回来后改的")).await.unwrap();
    a.drive().pull();
    until(|| a.applied() == 601).await;
    assert_same(&devices);
}

/// A drive in memory with commits 1 to `last` of generation `g`, all of them long ago, its snapshots
/// at `kept`, and `nexttodo.json` pointing to `pointer`.
fn old_drive(last: i64, kept: &[i64], pointer: &str) -> (Arc<Store>, View) {
    let store = Arc::new(Store::default());
    let view = View::new(store.clone());
    let long_ago = Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, 1_000_000, 0));
    for n in 1..=last {
        view.create(&super::commit::path("g", n), json!({ "id": long_ago, "format": 1, "items": [] }).to_string().as_bytes()).unwrap();
    }
    for n in kept {
        view.create(&format!("g/snapshots/{n}.db.gz"), b"x").unwrap();
    }
    view.create(setup::POINTER, &setup::pointer(pointer)).unwrap();
    (store, view)
}

#[test]
fn clearing_takes_the_commits_of_a_snapshot_that_was_never_kept_too() {
    // The one at 200 failed to upload.
    let (store, view) = old_drive(800, &[400, 600, 800], "g");
    super::clear(&view, "g", Clock::system().now()).unwrap();
    let left: Vec<i64> = log(&store).iter().map(|p| p.trim_start_matches("g/log/").trim_end_matches(".json").parse().unwrap()).collect();
    assert_eq!((left.len(), left.iter().min()), (400, Some(&401)));
    assert!(!store.paths().contains(&"g/snapshots/400.db.gz".to_owned()));
}

#[test]
fn generations_left_behind_are_cleared_only_by_a_device_of_the_one_nexttodo_json_points_to() {
    let other = Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, 1_000_000, 0)).to_string();
    for (pointer, kept) in [("g", false), (other.as_str(), true)] {
        let (store, view) = old_drive(0, &[], pointer);
        view.create(&format!("{other}/snapshots/0.db.gz"), b"x").unwrap();
        super::clear(&view, "g", Clock::system().now()).unwrap();
        assert_eq!(store.paths().contains(&format!("{other}/snapshots/0.db.gz")), kept, "pointing to {pointer}");
    }
}

#[test]
fn syncing_is_turned_on_in_an_empty_folder_or_one_of_nexttodos_own_only() {
    let store = Arc::new(Store::default());
    let view = View::new(store.clone());
    // Left by enabling cut short.
    view.create(&format!("{}/snapshots/0.db.gz", id()), b"x").unwrap();
    assert!(setup::probe(&view).unwrap().is_none());
    // Named by a UUID with a time in it too, yet not one of NextTodo's.
    for other in ["f81d4fae-7dec-11d0-a765-00a0c91e6bf6/a.txt", "照片/1.jpg"] {
        let view = View::new(Arc::new(Store::default()));
        view.create(other, b"x").unwrap();
        let Err(e) = setup::probe(&view) else { panic!("turned on in a folder with {other}") };
        assert_eq!(e.to_string(), "这个文件夹里有其他文件，请换一个空文件夹或新的文件夹名");
    }
}

#[test]
fn a_folder_or_commit_of_a_newer_format_asks_for_an_update_whatever_else_changed() {
    let newer = json!({ "format": 2, "generation": { "name": "g" }, "entries": [{ "kind": "unknown" }] }).to_string();
    assert!(matches!(setup::Pointer::read(newer.as_bytes()), Err(Trouble::NewerVersion)));
    let newer = json!({ "format": super::commit::FORMAT + 1, "id": id(), "entries": [] }).to_string();
    assert!(matches!(super::parse(newer.as_bytes()), Err(Trouble::NewerVersion)));
}

/// Commits of format 1 as this version wrote them, one of each kind of item and of most operations:
/// every later version reads them in, so a change to what commits hold that this would not read gets
/// a format of its own (docs/local.md#格式版本).
#[test]
fn commits_of_format_1_are_read_in() {
    let commits: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../../../fixtures/drive-commits-1.json")).unwrap();
    let (db, user) = (own(), "019a0000-0000-7000-8000-00000000a11c");
    db.hold(|c| {
        c.execute_batch(&format!("UPDATE users SET id = '{user}'; INSERT INTO sync_state (id, generation, applied) VALUES (1, 'g', 0);"))
            .unwrap()
    });
    for (n, commit) in (1..).zip(&commits) {
        let commit = super::parse(commit.to_string().as_bytes()).unwrap();
        db.hold(|c| super::read_in(c, user, commit, n, &HashMap::new(), &HashMap::new()).unwrap());
    }
    let text = |s: &str| Value::Text(s.into());
    assert_eq!(
        dump(&db, "SELECT title, done_at IS NOT NULL, due_date FROM tasks ORDER BY number"),
        [
            [text("订好搬家公司"), Value::Integer(1), text("2026-11-20")],
            // Repeating, done once: due a week on.
            [text("浇花"), Value::Integer(0), text("2026-11-26")],
            // Organized by AI.
            [text("设备 1"), Value::Integer(0), Value::Null],
        ]
    );
    // Both attachments went, one with its task, and their files and one never attached were cleared.
    assert_eq!(dump(&db, "SELECT count(*) FROM attachments"), [[Value::Integer(0)]]);
    assert_eq!(dump(&db, "SELECT count(*) FROM files WHERE purged"), [[Value::Integer(3)]]);
    assert_eq!(dump(&db, "SELECT text FROM ai_notes"), [[text("周末前搬完")]]);
    assert_eq!(dump(&db, "SELECT count(*) FROM mutations"), [[Value::Integer(15)]]);
}
