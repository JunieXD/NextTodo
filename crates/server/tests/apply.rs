//! The rules of changes as `fixtures/apply.json` has them; the web app applies the same cases locally
//! (apps/web/src/domain/apply-fixtures.test.ts). Names stand for ids: each is given one as it first
//! comes, and the snapshot is read back in names.

mod common;

use std::collections::{BTreeMap, HashMap};

use axum::http::StatusCode;
use common::{TestApp, id};
use nexttodo_core::model::{AiJobId, JobState};
use nexttodo_server::mutate::{self, Actor};
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize)]
struct Case {
    name: String,
    me: Option<String>,
    #[serde(default)]
    folders: Vec<(String, Option<String>)>,
    /// Folder, owner, members.
    #[serde(default)]
    shares: Vec<(String, String, Vec<String>)>,
    #[serde(default)]
    tasks: Vec<GivenTask>,
    #[serde(default)]
    deps: Vec<(String, String)>,
    /// Attachment, the task or AI job it belongs to.
    #[serde(default)]
    attachments: Vec<(String, String)>,
    /// AI job, folder, state.
    #[serde(default)]
    jobs: Vec<(String, Option<String>, JobState)>,
    ops: Vec<Value>,
    /// The day on the device the change is made on.
    today: Option<String>,
    error: Option<Value>,
    expect: Option<Map<String, Value>>,
}

/// Task, folder, assignees.
#[derive(Deserialize)]
struct GivenTask(String, Option<String>, #[serde(default)] Vec<String>);

/// The fields of operations that hold ids of things, and of users.
const THINGS: [&str; 8] = ["id", "folder_id", "parent_id", "task_id", "job_id", "before", "after", "mutation"];
const USERS: [&str; 4] = ["assign", "unassign", "assignees", "user_id"];

/// The test server with Alice, Bob and Carol, and the ids names stand for.
struct World {
    t: TestApp,
    secrets: HashMap<String, String>,
    ids: HashMap<String, String>,
    names: HashMap<String, String>,
}

impl World {
    async fn new() -> Self {
        let t = TestApp::new();
        let alice = t.admin().await;
        let mut w = World { secrets: HashMap::new(), ids: HashMap::new(), names: HashMap::new(), t };
        for name in ["bob", "carol"] {
            let secret = w.t.user(&alice, name).await;
            w.secrets.insert(name.into(), secret);
        }
        w.secrets.insert("alice".into(), alice);
        for name in ["alice", "bob", "carol"] {
            let user = w.t.me(&w.secrets[name]).await;
            w.named(name, user);
        }
        w
    }

    fn named(&mut self, name: &str, id: String) {
        self.names.insert(id.clone(), name.to_owned());
        self.ids.insert(name.to_owned(), id);
    }

    /// The id `name` stands for, a new one the first time.
    fn id(&mut self, name: &str) -> String {
        if !self.ids.contains_key(name) {
            self.named(name, id());
        }
        self.ids[name].clone()
    }

    /// `op` with ids in place of names.
    fn op(&mut self, op: &Value) -> Value {
        let mut op = op.clone();
        for (key, value) in op.as_object_mut().unwrap() {
            if THINGS.contains(&key.as_str()) || USERS.contains(&key.as_str()) {
                match value {
                    Value::String(name) => *value = json!(self.id(name)),
                    Value::Array(names) => *value = names.iter().map(|n| json!(self.id(n.as_str().unwrap()))).collect(),
                    _ => {}
                }
            }
        }
        op
    }

    async fn mutate(&mut self, by: &str, ops: &[Value], today: Option<&str>) -> (StatusCode, Value) {
        let ops: Vec<Value> = ops.iter().map(|op| self.op(op)).collect();
        let mut m = json!({ "id": id(), "ops": ops });
        if let Some(today) = today {
            m["today"] = today.into();
        }
        let r = self.t.call("POST", "/mutations", Some(&self.secrets[by]), Some(m)).await;
        (r.status, r.body)
    }

    async fn given(&mut self, by: &str, ops: Vec<Value>) {
        let (status, body) = self.mutate(by, &ops, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    fn name(&self, id: &Value) -> Value {
        id.as_str().map_or(Value::Null, |id| json!(self.names[id]))
    }

    /// What the case looks at of `user`'s data, in names.
    async fn seen(&self, user: &str) -> Map<String, Value> {
        let snap = self.t.snapshot(&self.secrets[user]).await;
        let all = |key: &str| snap[key].as_array().unwrap().clone();
        let by = |key: &str, of: &dyn Fn(&Value) -> Value| -> Value {
            all(key).iter().map(|v| (self.names[v["id"].as_str().unwrap()].clone(), of(v))).collect::<Map<_, _>>().into()
        };
        let mut deps: Vec<Value> = all("dependencies").iter().map(|d| json!([self.name(&d["before"]), self.name(&d["after"])])).collect();
        deps.sort_by_key(Value::to_string);
        let assignees: Map<String, Value> = all("tasks")
            .iter()
            .filter(|t| !t["assignees"].as_array().unwrap().is_empty())
            .map(|t| {
                let mut people: Vec<Value> = t["assignees"].as_array().unwrap().iter().map(|u| self.name(u)).collect();
                people.sort_by_key(Value::to_string);
                (self.names[t["id"].as_str().unwrap()].clone(), people.into())
            })
            .collect();
        let mut seen = Map::new();
        seen.insert("folders".into(), by("folders", &|f| self.name(&f["parent_id"])));
        seen.insert("tasks".into(), by("tasks", &|t| self.name(&t["folder_id"])));
        seen.insert("deps".into(), deps.into());
        seen.insert("assignees".into(), assignees.into());
        seen.insert(
            "attachments".into(),
            by("attachments", &|a| self.name(if a["task_id"].is_null() { &a["job_id"] } else { &a["task_id"] })),
        );
        seen.insert("jobs".into(), by("ai_jobs", &|j| j["state"].clone()));
        let due: Map<String, Value> = all("tasks")
            .iter()
            .filter(|t| !t["due"].is_null())
            .map(|t| (self.names[t["id"].as_str().unwrap()].clone(), t["due"]["date"].clone()))
            .collect();
        seen.insert("due".into(), due.into());
        seen
    }
}

/// Makes what the case gives, each thing by whoever has it: the owner of the shared folder it is
/// in, otherwise the user the case is about.
async fn give(w: &mut World, c: &Case, me: &str) {
    let shared: HashMap<&str, &str> = c.shares.iter().map(|(f, owner, _)| (f.as_str(), owner.as_str())).collect();
    let parents: HashMap<&str, Option<&str>> = c.folders.iter().map(|(f, p)| (f.as_str(), p.as_deref())).collect();
    let owner = |folder: Option<&str>| {
        let mut cur = folder;
        while let Some(f) = cur {
            if let Some(owner) = shared.get(f) {
                return owner.to_string();
            }
            cur = parents[f];
        }
        me.to_owned()
    };
    for (f, parent) in &c.folders {
        w.given(&owner(Some(f)), vec![json!({ "op": "folder.create", "id": f, "name": f, "parent_id": parent })]).await;
    }
    for (f, by, members) in &c.shares {
        let ops = members.iter().map(|m| json!({ "op": "share.add", "folder_id": f, "username": m })).collect();
        w.given(by, ops).await;
    }
    let mut task_owner = HashMap::new();
    for GivenTask(task, folder, assignees) in &c.tasks {
        let by = owner(folder.as_deref());
        let op = json!({ "op": "task.create", "id": task, "title": task, "folder_id": folder, "assignees": assignees });
        w.given(&by, vec![op]).await;
        task_owner.insert(task.as_str(), by);
    }
    for (before, after) in &c.deps {
        w.given(&task_owner[before.as_str()].clone(), vec![json!({ "op": "dep.add", "before": before, "after": after })]).await;
    }
    for (job, folder, _) in &c.jobs {
        let op = json!({ "op": "ai_job.create", "id": job, "text": "整理", "local_time": "2026-11-23 10:00 星期一", "folder_id": folder });
        w.given(me, vec![op]).await;
    }
    for (a, of) in &c.attachments {
        let by = task_owner.get(of.as_str()).cloned().unwrap_or_else(|| me.to_owned());
        let file = w.id(a);
        assert_eq!(w.t.upload(&w.secrets[&by], &file, "text/plain", b"1").await.status, StatusCode::OK);
        let holder = if task_owner.contains_key(of.as_str()) { "task_id" } else { "job_id" };
        let op = json!({ "op": "attachment.add", "id": a, "name": format!("{a}.txt"), "type": "text/plain", "size": 1, holder: of });
        w.given(&by, vec![op]).await;
    }
    for (job, _, state) in &c.jobs {
        // On as the server's AI organizing takes it: from waiting to running, and then on.
        let steps = match state {
            JobState::Waiting => vec![],
            JobState::Running => vec![(JobState::Waiting, JobState::Running)],
            _ => vec![(JobState::Waiting, JobState::Running), (JobState::Running, *state)],
        };
        let (user, id) = (w.ids[me].clone(), serde_json::from_value::<AiJobId>(json!(w.ids[job])).unwrap());
        let now = w.t.state.clock.now();
        w.t.state
            .db
            .call(move |c| {
                let actor = Actor { user: &user, token_id: "", via: Some("AI 整理") };
                for (from, to) in steps {
                    let error = (to == JobState::Failed).then(|| "模型接口出错".to_owned());
                    mutate::set_job_state(c, &actor, id, from, to, error, now)?;
                }
                Ok(())
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn changes_go_as_the_fixtures_say() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/apply.json")).unwrap();
    for c in cases {
        let mut w = World::new().await;
        let me = c.me.clone().unwrap_or_else(|| "alice".into());
        give(&mut w, &c, &me).await;
        let before = w.t.snapshot(&w.secrets[&me]).await;
        let (status, body) = w.mutate(&me, &c.ops, c.today.as_deref()).await;
        let wanted = match (c.error, c.expect) {
            (Some(error), None) => {
                assert!(!status.is_success(), "{}: {body}", c.name);
                assert_eq!(json!({ "code": body["code"], "message": body["message"] }), error, "{}", c.name);
                // Turned down, the batch leaves nothing behind, what came before the refused operation included.
                assert_eq!(w.t.snapshot(&w.secrets[&me]).await, before, "{}", c.name);
                continue;
            }
            (None, Some(expect)) if !expect.is_empty() => expect,
            _ => panic!("{}: either an error, or what to expect", c.name),
        };
        assert_eq!(status, StatusCode::OK, "{}: {body}", c.name);
        let seen = w.seen(&me).await;
        let wanted: BTreeMap<_, _> = wanted.into_iter().collect();
        for (what, mut value) in wanted {
            if what == "deps" {
                value.as_array_mut().unwrap().sort_by_key(Value::to_string);
            }
            assert_eq!(seen[&what], value, "{}: {what}", c.name);
        }
    }
}
