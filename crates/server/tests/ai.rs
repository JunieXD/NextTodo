//! AI organizing, with a stand-in for the model.

mod common;

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use common::{TestApp, id};
use nexttodo_core::model::{AiJobId, JobState};
use nexttodo_server::ai::llm::{Api, Config, Failure, Prompt};
use nexttodo_server::ai::secret::SecretKey;
use nexttodo_server::ai::{ACTOR, Ai, run_queue};
use nexttodo_server::mutate::{Actor, set_job_state};
use serde_json::{Value, json};

const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// Answers with what the test queued, in turn, and keeps what it was asked. The first chat waits
/// for the test when it holds one back.
#[derive(Default)]
struct Fake {
    answers: Mutex<VecDeque<Result<String, String>>>,
    asked: Mutex<Vec<(String, String)>>,
    held: Mutex<Option<Receiver<()>>>,
}

impl Fake {
    fn answer(&self, a: Result<&str, &str>) {
        self.answers.lock().unwrap().push_back(a.map(str::to_owned).map_err(str::to_owned));
    }

    /// Holds the next chat back until the sender is used.
    fn hold(&self) -> Sender<()> {
        let (tx, rx) = channel();
        *self.held.lock().unwrap() = Some(rx);
        tx
    }

    /// The keys and prompts it was asked with.
    fn asked(&self) -> Vec<(String, String)> {
        self.asked.lock().unwrap().clone()
    }
}

impl Api for Fake {
    fn chat(&self, c: &Config, prompt: &Prompt, json: bool, _: Duration) -> Result<String, Failure> {
        assert!(json);
        self.asked.lock().unwrap().push((c.api_key.clone(), prompt.user.clone()));
        let held = self.held.lock().unwrap().take();
        if let Some(rx) = held {
            rx.recv().unwrap();
        }
        let answer = self.answers.lock().unwrap().pop_front().expect("an answer queued");
        answer.map_err(|message| Failure { status: Some(401), message })
    }

    fn models(&self, _: &Config) -> Result<Vec<String>, Failure> {
        Ok(vec!["ecnu-max".into(), "ecnu-plus".into()])
    }
}

fn config() -> Value {
    json!({ "provider": "ecnu", "base_url": "https://chat.ecnu.edu.cn/open/api/v1", "model": "ecnu-plus", "thinking": "off", "api_key": "sk-test-1234" })
}

/// A server keeping keys with its own, and organizing jobs.
fn server() -> (TestApp, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let t = TestApp::with_ai(Ai::new(Some(SecretKey::from_base64(KEY).unwrap()), fake.clone()));
    tokio::spawn(run_queue(t.state.clone()));
    (t, fake)
}

fn create(job: &str, text: &str, folder: Option<&str>) -> Value {
    let mut op = json!({ "op": "ai_job.create", "id": job, "text": text, "local_time": "2026-11-04 16:30 星期三" });
    if let Some(f) = folder {
        op["folder_id"] = f.into();
    }
    op
}

/// The snapshot once `done` holds of it.
async fn until(t: &TestApp, s: &str, done: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..300 {
        let snap = t.snapshot(s).await;
        if done(&snap) {
            return snap;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out: {}", t.snapshot(s).await);
}

fn job_state(snap: &Value, job: &str) -> Option<(String, Value)> {
    let j = snap["ai_jobs"].as_array().unwrap().iter().find(|j| j["id"] == job)?;
    Some((j["state"].as_str().unwrap().to_owned(), j["error"].clone()))
}

fn titles(snap: &Value) -> Vec<&str> {
    let mut t: Vec<&str> = snap["tasks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect();
    t.sort();
    t
}

#[tokio::test]
async fn a_job_becomes_tasks_and_undoing_that_gives_it_back() {
    let (t, fake) = server();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "买材料", "folder": "/手工", "due": "2026-11-06" })).await;
    let folder = t.snapshot(&s).await["folders"][0]["id"].as_str().unwrap().to_owned();
    let put = t.call("PUT", "/ai/settings", Some(&s), Some(config())).await;
    assert_eq!(put.status, StatusCode::OK, "{}", put.body);
    assert_eq!(put.body["key_end"], "1234");

    fake.answer(Ok(r#"```json
{"tasks": [{"ref": "a", "title": "写初稿", "description": "- [ ] 引言", "after": [1]},
           {"title": "改稿", "after": ["a"], "due": "2026-11-05 15:00", "remind": 30}]}
```"#));
    let job = id();
    assert_eq!(t.mutate(&s, json!([create(&job, "先写初稿，写完明天下午三点前改好", Some(&folder))])).await.status, StatusCode::OK);
    let snap = until(&t, &s, |s| s["ai_jobs"].as_array().unwrap().is_empty()).await;
    assert_eq!(titles(&snap), ["买材料", "写初稿", "改稿"]);
    let task = |title: &str| snap["tasks"].as_array().unwrap().iter().find(|t| t["title"] == title).unwrap().clone();
    let (draft, revise) = (task("写初稿"), task("改稿"));
    assert_eq!(draft["folder_id"], folder.as_str());
    assert_eq!(draft["created_via"], ACTOR);
    assert_eq!(draft["description"], "- [ ] 引言");
    assert_eq!(revise["due"], json!({ "date": "2026-11-05", "time": "15:00", "remind_before": 30 }));
    let deps: Vec<(Value, Value)> =
        snap["dependencies"].as_array().unwrap().iter().map(|d| (d["before"].clone(), d["after"].clone())).collect();
    assert!(deps.contains(&(task("买材料")["id"].clone(), draft["id"].clone())));
    assert!(deps.contains(&(draft["id"].clone(), revise["id"].clone())));

    // What the model was shown: the time, the folder and its open tasks, and the text.
    let (key, shown) = fake.asked().pop().unwrap();
    assert_eq!(key, "sk-test-1234");
    let shown: Value = serde_json::from_str(&shown).unwrap();
    assert_eq!(shown["local_time"], "2026-11-04 16:30 星期三");
    assert_eq!(shown["folder"], "/手工");
    assert_eq!(shown["open_tasks"], json!([{ "number": 1, "title": "买材料", "due": "2026-11-06" }]));
    assert_eq!(shown["text"], "先写初稿，写完明天下午三点前改好");

    // The job's course is in the log as the AI's: started, then done in one change.
    let log = t.call("GET", "/changes?since=0", Some(&s), None).await.body;
    let ai: Vec<&Value> = log["changes"].as_array().unwrap().iter().filter(|c| c["actor"] == ACTOR).collect();
    assert_eq!(ai.len(), 2);
    let done = ai[1];
    assert!(done["effects"].as_array().unwrap().iter().any(|e| e["kind"] == "ai_job" && e["after"].is_null()));

    // Undone, the tasks go and the job is back, failed so that nothing comes of it by itself.
    assert_eq!(t.mutate(&s, json!([{ "op": "undo", "mutation": done["id"] }])).await.status, StatusCode::OK);
    let snap = t.snapshot(&s).await;
    assert_eq!(titles(&snap), ["买材料"]);
    assert_eq!(job_state(&snap, &job), Some(("failed".into(), json!("已撤销"))));

    // Retried with the text changed, it is organized again.
    fake.answer(Ok(r#"{"tasks": [{"title": "写初稿"}]}"#));
    let retry = t.mutate(&s, json!([{ "op": "ai_job.retry", "id": job, "text": "写初稿" }])).await;
    assert_eq!(retry.status, StatusCode::OK, "{}", retry.body);
    let snap = until(&t, &s, |s| s["ai_jobs"].as_array().unwrap().is_empty()).await;
    assert_eq!(titles(&snap), ["买材料", "写初稿"]);
    assert!(fake.asked().pop().unwrap().1.contains("\"text\": \"写初稿\""));
}

#[tokio::test]
async fn a_failed_job_says_why_and_can_be_retried() {
    let (t, fake) = server();
    let s = t.admin().await;
    t.call("PUT", "/ai/settings", Some(&s), Some(config())).await;
    let job = id();
    let failed = |reason: &'static str| {
        let job = job.clone();
        move |s: &Value| job_state(s, &job) == Some(("failed".into(), json!(reason)))
    };
    fake.answer(Ok("好的，我来整理"));
    t.mutate(&s, json!([create(&job, "随便写点", None)])).await;
    until(&t, &s, failed("模型返回的不是合法的 JSON")).await;

    fake.answer(Err("接口返回 401：invalid api key"));
    t.mutate(&s, json!([{ "op": "ai_job.retry", "id": job }])).await;
    until(&t, &s, failed("接口返回 401：invalid api key")).await;

    fake.answer(Ok(r#"{"tasks": [{"title": "接着做", "after": [99]}]}"#));
    t.mutate(&s, json!([{ "op": "ai_job.retry", "id": job }])).await;
    until(&t, &s, failed("结果引用了不在这个文件夹里的事项 #99")).await;

    // A plan the rules turn down fails whole.
    fake.answer(Ok(r#"{"tasks": [{"ref": "a", "title": "甲", "after": ["b"]}, {"ref": "b", "title": "乙", "after": ["a"]}]}"#));
    t.mutate(&s, json!([{ "op": "ai_job.retry", "id": job }])).await;
    let snap =
        until(&t, &s, |s| job_state(s, &job).is_some_and(|(state, e)| state == "failed" && e.as_str().unwrap().contains("循环"))).await;
    assert!(titles(&snap).is_empty());

    // Only a failed job is retried; a deleted one is gone.
    let snap = t.snapshot(&s).await;
    assert_eq!(job_state(&snap, &job).unwrap().0, "failed");
    t.mutate(&s, json!([{ "op": "ai_job.delete", "id": job }])).await;
    assert_eq!(t.mutate(&s, json!([{ "op": "ai_job.retry", "id": job }])).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_device_hands_its_settings_over_for_one_job() {
    let (t, fake) = server();
    let s = t.admin().await;
    let job = id();
    t.mutate(&s, json!([create(&job, "整理一下", None)])).await;
    // No settings on the server: it waits for a device.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(job_state(&t.snapshot(&s).await, &job).unwrap().0, "waiting");
    assert!(fake.asked().is_empty());

    let run = format!("/ai/jobs/{job}/run");
    let mut bad = config();
    bad["base_url"] = "http://192.168.1.1/v1".into();
    assert_eq!(t.call("POST", &run, Some(&s), Some(bad)).await.status, StatusCode::BAD_REQUEST);
    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "Claude Code", "scope": "write" }))).await.body;
    let token = token["secret"].as_str().unwrap();
    assert_eq!(t.call("POST", &run, Some(token), Some(config())).await.status, StatusCode::FORBIDDEN);
    assert_eq!(t.call("GET", "/ai/settings", Some(token), None).await.status, StatusCode::FORBIDDEN);

    fake.answer(Ok(r#"{"tasks": [{"title": "整理一下"}]}"#));
    assert_eq!(t.call("POST", &run, Some(&s), Some(config())).await.status, StatusCode::ACCEPTED);
    until(&t, &s, |s| s["ai_jobs"].as_array().unwrap().is_empty()).await;
    assert_eq!(fake.asked()[0].0, "sk-test-1234");
    // Nothing of the key stays on the server.
    assert_eq!(t.call("GET", "/ai/settings", Some(&s), None).await.body, Value::Null);
    assert_eq!(t.call("POST", &run, Some(&s), Some(config())).await.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn deleting_a_job_being_organized_throws_its_result_away() {
    let (t, fake) = server();
    let s = t.admin().await;
    t.call("PUT", "/ai/settings", Some(&s), Some(config())).await;
    let release = fake.hold();
    fake.answer(Ok(r#"{"tasks": [{"title": "甲"}]}"#));
    fake.answer(Ok(r#"{"tasks": [{"title": "乙"}]}"#));
    let (first, second) = (id(), id());
    t.mutate(&s, json!([create(&first, "甲", None)])).await;
    until(&t, &s, |s| job_state(s, &first).is_some_and(|(state, _)| state == "running")).await;
    let retry = t.mutate(&s, json!([{ "op": "ai_job.retry", "id": first }])).await;
    assert_eq!(retry.body["message"], "正在整理，稍后再改");
    let deleted = t.mutate(&s, json!([{ "op": "ai_job.delete", "id": first }])).await;
    assert_eq!(deleted.status, StatusCode::OK);
    // The next waits for the user's turn, which ends with the first.
    t.mutate(&s, json!([create(&second, "乙", None)])).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(job_state(&t.snapshot(&s).await, &second).unwrap().0, "waiting");
    release.send(()).unwrap();
    let snap = until(&t, &s, |s| s["ai_jobs"].as_array().unwrap().is_empty()).await;
    assert_eq!(titles(&snap), ["乙"]);
}

#[tokio::test]
async fn undoing_the_deletion_of_a_job_being_organized_brings_it_back_failed() {
    let t = TestApp::new();
    let s = t.admin().await;
    let job = id();
    t.mutate(&s, json!([create(&job, "整理", None)])).await;
    let owner = t.me(&s).await;
    let job_id: AiJobId = serde_json::from_value(json!(job)).unwrap();
    let now = t.state.clock.now();
    t.state
        .db
        .call(move |c| {
            set_job_state(
                c,
                &Actor { owner: &owner, token_id: "", via: Some(ACTOR) },
                job_id,
                JobState::Waiting,
                JobState::Running,
                None,
                now,
            )
        })
        .await
        .unwrap()
        .unwrap();
    let delete = id();
    t.call("POST", "/mutations", Some(&s), Some(json!({ "id": delete, "ops": [{ "op": "ai_job.delete", "id": job }] }))).await;
    t.mutate(&s, json!([{ "op": "undo", "mutation": delete }])).await;
    assert_eq!(job_state(&t.snapshot(&s).await, &job), Some(("failed".into(), json!("已撤销"))));
}

#[tokio::test]
async fn a_restart_fails_the_jobs_it_cut_short() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (cut, waiting) = (id(), id());
    t.mutate(&s, json!([create(&cut, "整理", None), create(&waiting, "等着", None)])).await;
    let owner = t.me(&s).await;
    let job_id: AiJobId = serde_json::from_value(json!(cut)).unwrap();
    let now = t.state.clock.now();
    t.state
        .db
        .call(move |c| {
            set_job_state(
                c,
                &Actor { owner: &owner, token_id: "", via: Some(ACTOR) },
                job_id,
                JobState::Waiting,
                JobState::Running,
                None,
                now,
            )
        })
        .await
        .unwrap();
    tokio::spawn(run_queue(t.state.clone()));
    let snap = until(&t, &s, |s| job_state(s, &cut).unwrap().0 == "failed").await;
    assert_eq!(job_state(&snap, &cut).unwrap().1, "服务器重启，整理中断");
    assert_eq!(job_state(&snap, &waiting).unwrap().0, "waiting");
}

#[tokio::test]
async fn settings_are_each_users_own_and_the_key_stays_sealed() {
    let plain = TestApp::new();
    let s = plain.admin().await;
    assert_eq!(plain.call("GET", "/ai/providers", Some(&s), None).await.body["sync"], false);
    assert_eq!(plain.call("PUT", "/ai/settings", Some(&s), Some(config())).await.status, StatusCode::CONFLICT);

    let (t, _) = server();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    let providers = t.call("GET", "/ai/providers", Some(&alice), None).await.body;
    assert_eq!(providers["sync"], true);
    assert_eq!(providers["providers"][0]["id"], "ecnu");
    t.call("PUT", "/ai/settings", Some(&alice), Some(config())).await;
    assert_eq!(t.call("GET", "/ai/settings", Some(&bob), None).await.body, Value::Null);
    let without_key =
        json!({ "provider": "ecnu", "base_url": "https://chat.ecnu.edu.cn/open/api/v1", "model": "ecnu-max", "thinking": "high" });
    assert_eq!(t.call("PUT", "/ai/settings", Some(&bob), Some(without_key.clone())).await.status, StatusCode::BAD_REQUEST);
    let kept = t.call("PUT", "/ai/settings", Some(&alice), Some(without_key)).await.body;
    assert_eq!(
        kept,
        json!({ "provider": "ecnu", "base_url": "https://chat.ecnu.edu.cn/open/api/v1", "model": "ecnu-max", "thinking": "high", "key_end": "1234" })
    );
    let models = t
        .call(
            "POST",
            "/ai/models",
            Some(&alice),
            Some(json!({ "provider": "ecnu", "base_url": "https://chat.ecnu.edu.cn/open/api/v1", "model": "x", "thinking": "off" })),
        )
        .await;
    assert_eq!(models.body["models"], json!(["ecnu-max", "ecnu-plus"]));

    let sealed: Vec<u8> = t.state.db.call(|c| Ok(c.query_row("SELECT api_key FROM ai_settings", [], |r| r.get(0))?)).await.unwrap();
    assert!(!sealed.windows(4).any(|w| w == b"1234"));

    assert_eq!(t.call("DELETE", "/ai/settings", Some(&alice), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(t.call("GET", "/ai/settings", Some(&alice), None).await.body, Value::Null);
}

#[tokio::test]
async fn jobs_go_with_their_folder_and_are_each_users_own() {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    t.ok(&alice, "mkdir", json!({ "path": "/读书" })).await;
    let folder = t.snapshot(&alice).await["folders"][0]["id"].as_str().unwrap().to_owned();
    let job = id();
    t.mutate(&alice, json!([create(&job, "读完第三章", Some(&folder))])).await;

    assert_eq!(t.mutate(&bob, json!([{ "op": "ai_job.delete", "id": job }])).await.status, StatusCode::NOT_FOUND);
    assert_eq!(t.mutate(&bob, json!([create(&id(), "x", Some(&folder))])).await.status, StatusCode::NOT_FOUND);
    assert!(t.snapshot(&bob).await["ai_jobs"].as_array().unwrap().is_empty());
    assert_eq!(t.mutate(&alice, json!([create(&id(), "  ", None)])).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(t.mutate(&alice, json!([{ "op": "ai_job.retry", "id": job }])).await.body["message"], "这条请求已在等待整理");

    let delete = id();
    t.call("POST", "/mutations", Some(&alice), Some(json!({ "id": delete, "ops": [{ "op": "folder.delete", "id": folder }] }))).await;
    assert!(t.snapshot(&alice).await["ai_jobs"].as_array().unwrap().is_empty());
    t.mutate(&alice, json!([{ "op": "undo", "mutation": delete }])).await;
    assert_eq!(job_state(&t.snapshot(&alice).await, &job).unwrap().0, "waiting");
}

#[tokio::test]
async fn agents_see_the_jobs_and_how_they_stand() {
    let t = TestApp::new();
    let s = t.admin().await;
    assert_eq!(t.ok(&s, "jobs", json!({})).await["text"], "没有 AI 整理请求。");
    t.ok(&s, "mkdir", json!({ "path": "/读书" })).await;
    let folder = t.snapshot(&s).await["folders"][0]["id"].as_str().unwrap().to_owned();
    let job = id();
    t.mutate(&s, json!([create(&job, "读完第三章，然后写一篇读书笔记\n第二行", Some(&folder))])).await;
    let jobs = t.ok(&s, "jobs", json!({})).await;
    assert_eq!(jobs["text"], "“读完第三章，然后写一篇读书笔记…” · /读书 · 等待整理 · 刚刚");
    assert_eq!(jobs["data"][0]["state"], "waiting");
    assert_eq!(jobs["data"][0]["id"], job.as_str());
    assert_eq!(jobs["data"][0].get("error"), None);
    let list = t.ok(&s, "list", json!({ "folder": "/读书" })).await;
    assert_eq!(list["text"], "/读书\n  AI 整理：等待 1（用 jobs 查看）");
    assert_eq!(list["data"]["jobs"], json!({ "waiting": 1 }));
    assert_eq!(t.ok(&s, "list", json!({})).await["data"].get("jobs"), None, "only the folder's own");
    let log = t.ok(&s, "log", json!({})).await;
    assert!(log["text"].as_str().unwrap().contains("提交 AI 整理 “读完第三章，然后写一篇读书笔记…”"), "{}", log["text"]);
}

#[tokio::test]
async fn an_answer_too_big_for_one_change_fails() {
    let (t, fake) = server();
    let s = t.admin().await;
    t.call("PUT", "/ai/settings", Some(&s), Some(config())).await;
    let tasks: Vec<Value> = (0..1000).map(|i| json!({ "title": format!("第 {i} 项") })).collect();
    fake.answer(Ok(&json!({ "tasks": tasks }).to_string()));
    let job = id();
    t.mutate(&s, json!([create(&job, "很多事", None)])).await;
    let snap = until(&t, &s, |s| job_state(s, &job).is_some_and(|(state, _)| state == "failed")).await;
    assert_eq!(job_state(&snap, &job).unwrap().1, "每次最多 1000 个操作，本次有 1001 个");
    assert!(titles(&snap).is_empty());
}
