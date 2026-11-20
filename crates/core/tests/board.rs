//! Checks `Board` against the fixtures shared with the web app.

use std::collections::{BTreeMap, HashMap};

use nexttodo_core::board::{Board, Now, Snapshot, Status};
use nexttodo_core::calendar;
use nexttodo_core::model::{Date, FolderId, Task, Time};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Case {
    name: String,
    /// The user's today; any day for the cases without start dates.
    #[serde(default)]
    today: Option<String>,
    /// The time there, if known.
    #[serde(default)]
    time: Option<String>,
    folders: Vec<Value>,
    tasks: Vec<Value>,
    deps: Vec<(String, String)>,
    /// Shared folders: `{folder, owner, members}`, people by username; the user is `me`.
    #[serde(default)]
    shares: Vec<Value>,
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
    /// The tasks in a folder and its subfolders, by name.
    #[serde(default)]
    within: HashMap<String, Vec<String>>,
    #[serde(default)]
    steps: HashMap<String, Option<(usize, usize)>>,
    /// `2026-11-12` or `2026-11-12 20:00`.
    next_start: Option<String>,
    agenda: Option<AgendaWant>,
}

#[derive(Deserialize)]
struct AgendaWant {
    days: u32,
    overdue: Vec<String>,
    /// By day, a time a repeating task is due later marked `+`.
    due: BTreeMap<String, Vec<String>>,
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
                "title": t["id"], "description": "", "done_at": t["done_at"], "start": t["start"], "start_time": t["start_time"], "due": t["due"], "completions": t["completions"], "sort_key": t["key"],
                "assignees": people(names, &t["assignees"]),
                "created_at": 0, "updated_at": 0, "created_by": names.id("me"), "updated_by": names.id("me"), "created_via": null, "updated_via": null,
            })
        })
        .collect();
    let dependencies: Vec<Value> =
        case.deps.iter().map(|(b, a)| json!({ "before": names.id(b), "after": names.id(a), "created_at": 0 })).collect();
    let shares: Vec<Value> = case
        .shares
        .iter()
        .map(|sh| json!({ "folder_id": names.id(sh["folder"].as_str().unwrap()), "owner": names.id(sh["owner"].as_str().unwrap()), "members": people(names, &sh["members"]) }))
        .collect();
    let mut usernames = vec!["me"];
    for sh in &case.shares {
        usernames.extend(std::iter::once(&sh["owner"]).chain(sh["members"].as_array().unwrap()).map(|u| u.as_str().unwrap()));
    }
    usernames.sort();
    usernames.dedup();
    let people: Vec<Value> = usernames.into_iter().map(|u| json!({ "id": names.id(u), "username": u, "display_name": u })).collect();
    serde_json::from_value(
        json!({ "folders": folders, "tasks": tasks, "dependencies": dependencies, "ai_jobs": [], "shares": shares, "people": people }),
    )
    .unwrap()
}

/// The ids of people given by username, in order.
fn people(names: &mut Names, list: &Value) -> Vec<String> {
    let mut ids: Vec<String> = list.as_array().into_iter().flatten().map(|u| names.id(u.as_str().unwrap())).collect();
    ids.sort();
    ids
}

#[test]
fn board_matches_fixtures() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/board.json")).unwrap();
    for case in &cases {
        let mut names = Names::default();
        let snap = snapshot(case, &mut names);
        let today = Date::try_from(case.today.clone().unwrap_or_else(|| "2026-11-12".into())).unwrap();
        let now = Now { today, time: case.time.clone().map(|t| Time::try_from(t).unwrap()) };
        let me = names.id("me");
        let board = Board::new(&snap, now, &me);
        let task = |names: &mut Names, n: &str| board.task(serde_json::from_value(json!(names.id(n))).unwrap()).unwrap();
        let folder =
            |names: &mut Names, n: &str| -> Option<FolderId> { (n != "/").then(|| serde_json::from_value(json!(names.id(n))).unwrap()) };
        let list = |names: &Names, tasks: Vec<&Task>| tasks.into_iter().map(|t| names.name(t)).collect::<Vec<_>>();
        let c = &case.name;
        for (n, want) in &case.expect.status {
            let got = match board.status(task(&mut names, n)) {
                Status::Ready => "ready".to_owned(),
                Status::Waiting { on, not_started: false } => format!("waiting {on}"),
                Status::Waiting { on: 0, not_started: true } => "not started".to_owned(),
                Status::Waiting { on, not_started: true } => format!("waiting {on}, not started"),
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
        let next_start =
            board.next_start().map(|(d, t)| t.map_or_else(|| d.as_str().to_owned(), |t| format!("{} {}", d.as_str(), t.as_str())));
        assert_eq!(next_start, case.expect.next_start, "{c}: next start");
        let steps = board.steps();
        for (n, want) in &case.expect.steps {
            assert_eq!(steps.get(&task(&mut names, n).id).copied(), *want, "{c}: steps of {n}");
        }
        for (f, want) in &case.expect.paths {
            let f = folder(&mut names, f);
            assert_eq!(&board.path(f), want, "{c}: path");
            assert_eq!(board.find_folder(want), Some(f), "{c}: find {want}");
        }
        if let Some(want) = &case.expect.agenda {
            let got = calendar::agenda(&board, want.days);
            assert_eq!(list(&names, got.overdue), want.overdue, "{c}: overdue");
            let due: BTreeMap<String, Vec<String>> = got
                .days
                .into_iter()
                .map(|(day, list)| {
                    (day.as_str().to_owned(), list.iter().map(|d| names.name(d.task) + if d.later { "+" } else { "" }).collect())
                })
                .collect();
            assert_eq!(due, want.due, "{c}: agenda");
        }
        for (f, want) in &case.expect.within {
            let f = folder(&mut names, f);
            let mut got = list(&names, board.within(f));
            got.sort();
            assert_eq!(&got, want, "{c}: within {f:?}");
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
    let case = Case {
        name: String::new(),
        today: None,
        time: None,
        folders,
        tasks,
        deps,
        shares: Vec::new(),
        expect: serde_json::from_value(json!({})).unwrap(),
    };
    let mut names = Names::default();
    let snap = snapshot(&case, &mut names);
    let board = Board::new(&snap, Now { today: Date::try_from("2026-11-12".to_owned()).unwrap(), time: None }, "me");
    let f = Some(serde_json::from_value(json!(names.id("f"))).unwrap());
    let order: Vec<String> = board.tasks_in(f).into_iter().map(|t| names.name(t)).collect();
    let want: Vec<String> = (0..n).map(|i| format!("b{i}")).chain((0..n).map(|i| format!("a{i}"))).collect();
    assert_eq!(order, want);
}
