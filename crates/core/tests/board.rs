//! Checks `Board` against the fixtures shared with the web app.

use std::collections::HashMap;

use nexttodo_core::board::{Board, Snapshot, Status};
use nexttodo_core::model::{FolderId, Task};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Case {
    name: String,
    folders: Vec<Value>,
    tasks: Vec<Value>,
    deps: Vec<(String, String)>,
    expect: Expect,
}

#[derive(Deserialize)]
struct Expect {
    #[serde(default)]
    status: HashMap<String, String>,
    #[serde(default)]
    unlocks: HashMap<String, Vec<String>>,
    #[serde(default)]
    order: HashMap<String, Vec<String>>,
    #[serde(default)]
    next: Vec<String>,
    #[serde(default)]
    paths: HashMap<String, String>,
}

/// Turns the short names in a fixture into entity ids, and back.
#[derive(Default)]
struct Names(Vec<String>);

impl Names {
    fn id(&mut self, name: &str) -> String {
        let i = self.0.iter().position(|n| n == name).unwrap_or_else(|| {
            self.0.push(name.to_owned());
            self.0.len() - 1
        });
        format!("00000000-0000-7000-8000-{:012}", i + 1)
    }

    fn name(&self, t: &Task) -> String {
        t.title.as_str().to_owned()
    }
}

fn snapshot(case: &Case, names: &mut Names) -> Snapshot {
    let s = |v: &Value| v.as_str().map(str::to_owned);
    let folders: Vec<Value> = case
        .folders
        .iter()
        .map(|f| {
            json!({
                "id": names.id(f["id"].as_str().unwrap()),
                "parent_id": s(&f["parent"]).map(|p| names.id(&p)),
                "name": f["name"], "sort_key": f["key"], "created_at": 0, "updated_at": 0,
            })
        })
        .collect();
    let tasks: Vec<Value> = case
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            json!({
                "id": names.id(t["id"].as_str().unwrap()), "number": i + 1,
                "folder_id": s(&t["folder"]).map(|f| names.id(&f)),
                "title": t["id"], "description": "", "done_at": t["done_at"], "due": t["due"], "sort_key": t["key"],
                "created_at": 0, "updated_at": 0, "created_via": null, "updated_via": null,
            })
        })
        .collect();
    let dependencies: Vec<Value> =
        case.deps.iter().map(|(b, a)| json!({ "before": names.id(b), "after": names.id(a), "created_at": 0 })).collect();
    serde_json::from_value(json!({ "folders": folders, "tasks": tasks, "dependencies": dependencies })).unwrap()
}

#[test]
fn board_matches_fixtures() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/board.json")).unwrap();
    for case in &cases {
        let mut names = Names::default();
        let snap = snapshot(case, &mut names);
        let board = Board::new(&snap);
        let task = |names: &mut Names, n: &str| board.task(serde_json::from_value(json!(names.id(n))).unwrap()).unwrap();
        let folder =
            |names: &mut Names, n: &str| -> Option<FolderId> { (n != "/").then(|| serde_json::from_value(json!(names.id(n))).unwrap()) };
        let list = |names: &Names, tasks: Vec<&Task>| tasks.into_iter().map(|t| names.name(t)).collect::<Vec<_>>();
        let c = &case.name;
        for (n, want) in &case.expect.status {
            let got = match board.status(task(&mut names, n)) {
                Status::Ready => "ready".to_owned(),
                Status::Waiting(k) => format!("waiting {k}"),
                Status::Done => "done".to_owned(),
            };
            assert_eq!(&got, want, "{c}: status of {n}");
        }
        for (n, want) in &case.expect.unlocks {
            let t = task(&mut names, n);
            assert_eq!(&list(&names, board.unlocks(t)), want, "{c}: unlocks of {n}");
        }
        for (f, want) in &case.expect.order {
            let f = folder(&mut names, f);
            assert_eq!(&list(&names, board.tasks_in(f)), want, "{c}: order in {f:?}");
        }
        assert_eq!(list(&names, board.next()), case.expect.next, "{c}: next");
        for (f, want) in &case.expect.paths {
            let f = folder(&mut names, f);
            assert_eq!(&board.path(f), want, "{c}: path");
            assert_eq!(board.find_folder(want), Some(f), "{c}: find {want}");
        }
    }
}

#[test]
fn ordering_stays_linear_through_a_shared_task() {
    // Many tasks in one folder go through a single task elsewhere: n + n edges, not n × n.
    let n = 3000;
    let mut tasks: Vec<Value> = (0..n).map(|i| json!({ "id": format!("b{i}"), "folder": "f", "key": "a0" })).collect();
    tasks.push(json!({ "id": "hub", "folder": "g", "key": "a0" }));
    tasks.extend((0..n).map(|i| json!({ "id": format!("a{i}"), "folder": "f", "key": "a0" })));
    let deps = (0..n).flat_map(|i| [(format!("b{i}"), "hub".to_owned()), ("hub".to_owned(), format!("a{i}"))]).collect();
    let folders = vec![json!({ "id": "f", "name": "f", "key": "a0" }), json!({ "id": "g", "name": "g", "key": "a1" })];
    let case = Case { name: String::new(), folders, tasks, deps, expect: serde_json::from_value(json!({})).unwrap() };
    let mut names = Names::default();
    let snap = snapshot(&case, &mut names);
    let board = Board::new(&snap);
    let f = Some(serde_json::from_value(json!(names.id("f"))).unwrap());
    let order: Vec<String> = board.tasks_in(f).into_iter().map(|t| names.name(t)).collect();
    let want: Vec<String> = (0..n).map(|i| format!("b{i}")).chain((0..n).map(|i| format!("a{i}"))).collect();
    assert_eq!(order, want);
}
