//! Shared folders: who sees what, who gets which changes, and what moving in and out does.

use crate::common::{TestApp, id};
use axum::http::StatusCode;
use serde_json::{Value, json};

/// Alice, who shares `/装修` (holding `/装修/水电` and the task “量尺寸”) with Bob; Carol, who is
/// not in it; and Alice's private folder `/私人`.
struct Shared {
    t: TestApp,
    alice: String,
    bob: String,
    carol: String,
    ids: Ids,
    home: String,
    private: String,
}

struct Ids {
    alice: String,
    bob: String,
    carol: String,
}

async fn shared() -> Shared {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    let carol = t.user(&alice, "carol").await;
    let ids = Ids { alice: t.me(&alice).await, bob: t.me(&bob).await, carol: t.me(&carol).await };
    let (home, private) = (id(), id());
    let r = t
        .mutate(
            &alice,
            json!([
                { "op": "folder.create", "id": home, "name": "装修" },
                { "op": "folder.create", "id": id(), "name": "水电", "parent_id": home },
                { "op": "folder.create", "id": private, "name": "私人" },
                { "op": "task.create", "id": id(), "title": "量尺寸", "folder_id": home },
                { "op": "task.create", "id": id(), "title": "日记", "folder_id": private },
            ]),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    t.ok(&alice, "share", json!({ "folder": "/装修", "users": ["bob"] })).await;
    Shared { t, alice, bob, carol, ids, home, private }
}

fn titles(snap: &Value) -> Vec<&str> {
    snap["tasks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect()
}

fn task<'a>(snap: &'a Value, title: &str) -> &'a Value {
    snap["tasks"].as_array().unwrap().iter().find(|t| t["title"] == title).unwrap_or_else(|| panic!("no task {title}"))
}

fn text(reply: &Value) -> &str {
    reply["text"].as_str().unwrap()
}

/// The batch a write tool made.
fn batch(reply: &Value) -> i64 {
    reply["data"]["batch"].as_i64().unwrap()
}

async fn seq(t: &TestApp, secret: &str) -> i64 {
    t.snapshot(secret).await["seq"].as_i64().unwrap()
}

#[tokio::test]
async fn members_see_the_shared_folder_and_nothing_else() {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    let bob_id = t.me(&bob).await;
    let home = id();
    t.mutate(
        &alice,
        json!([
            { "op": "folder.create", "id": home, "name": "装修" },
            { "op": "task.create", "id": id(), "title": "量尺寸", "folder_id": home },
            { "op": "task.create", "id": id(), "title": "日记" },
        ]),
    )
    .await;
    let before = seq(&t, &bob).await;
    let r = t.ok(&alice, "share", json!({ "folder": "/装修", "users": ["bob"] })).await;
    assert!(text(&r).contains("添加成员"), "{}", text(&r));

    let snap = t.snapshot(&bob).await;
    assert_eq!(titles(&snap), ["量尺寸"]);
    assert_eq!(snap["folders"][0]["name"], "装修");
    assert_eq!(snap["shares"], json!([{ "folder_id": home, "owner": t.me(&alice).await, "members": [bob_id] }]));
    let mut people: Vec<&str> = snap["people"].as_array().unwrap().iter().map(|p| p["username"].as_str().unwrap()).collect();
    people.sort();
    assert_eq!(people, ["alice", "bob"]);
    // What Bob had is no base to go on from: it lacks everything in the folder.
    let r = t.call("GET", &format!("/changes?since={before}"), Some(&bob), None).await;
    assert_eq!((r.status, r.body["code"].as_str()), (StatusCode::GONE, Some("resync")));

    let tree = t.ok(&bob, "tree", json!({})).await;
    assert!(text(&tree).contains("@alice/装修（待办 1，可做 1） · 共享人 alice，成员 bob"), "{}", text(&tree));
}

#[tokio::test]
async fn members_work_in_the_folder_with_its_owners_numbers() {
    let s = shared().await;
    let t = &s.t;
    let since = seq(t, &s.alice).await;
    // Bob numbers his own tasks; the folder's are Alice's.
    t.ok(&s.bob, "add", json!({ "title": "自己的" })).await;
    let added = t.ok(&s.bob, "add", json!({ "title": "买电线", "folder": "@alice/装修/水电" })).await;
    assert!(text(&added).contains("@alice#3 买电线"), "{}", text(&added));

    let snap = t.snapshot(&s.alice).await;
    let wire = task(&snap, "买电线");
    assert_eq!((wire["number"].as_i64(), wire["created_by"].as_str()), (Some(3), Some(s.ids.bob.as_str())));
    assert!(!titles(&snap).contains(&"自己的"));
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&s.alice), None).await.body;
    let changes = changes["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "Alice gets Bob's change in her folder, not the one in his own: {changes:?}");
    assert_eq!(changes[0]["user"], s.ids.bob.as_str());

    let shown = t.ok(&s.alice, "show", json!({ "task": 3 })).await;
    assert!(text(&shown).contains("创建：刚刚，bob"), "{}", text(&shown));
    assert_eq!(shown["data"]["created_by"], "bob");
    let log = t.ok(&s.alice, "log", json!({})).await;
    assert!(text(&log).lines().next().unwrap().ends_with("· bob"), "{}", text(&log));
    assert_eq!(log["data"][0]["user"], "bob");
    t.ok(&s.bob, "done", json!({ "tasks": ["@alice#3"] })).await;
    assert!(task(&t.snapshot(&s.alice).await, "买电线")["done_at"].is_i64());
    // Carol is in none of it.
    let carol = t.tool(&s.carol, "show", json!({ "task": "@alice#3" })).await;
    assert_eq!(carol.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn links_and_assignees_stay_inside_the_folder() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.bob, "add", json!({ "title": "自己的" })).await;
    let r = t.tool(&s.bob, "dep_add", json!({ "before": 1, "after": "@alice#1" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.body["message"].as_str().unwrap().starts_with("只能连同一处的事项"), "{}", r.body);

    let assigned = t.ok(&s.alice, "edit", json!({ "task": 1, "assign": ["bob"] })).await;
    assert!(text(&assigned).contains("修改"), "{}", text(&assigned));
    // Assigning adds to whoever is assigned by then.
    t.ok(&s.bob, "edit", json!({ "task": "@alice#1", "assign": ["alice"] })).await;
    let mut both: Vec<String> = serde_json::from_value(task(&t.snapshot(&s.alice).await, "量尺寸")["assignees"].clone()).unwrap();
    both.sort();
    let mut want = vec![s.ids.alice.clone(), s.ids.bob.clone()];
    want.sort();
    assert_eq!(both, want);
    let shown = t.ok(&s.bob, "show", json!({ "task": "@alice#1" })).await;
    assert!(text(&shown).contains("负责：") && text(&shown).contains("bob"), "{}", text(&shown));

    let r = t.tool(&s.alice, "edit", json!({ "task": 1, "assign": ["carol"] })).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "Alice shares nothing with Carol: {}", r.body);
    let r = t
        .mutate(
            &s.alice,
            json!([{ "op": "task.update", "id": task(&t.snapshot(&s.alice).await, "量尺寸")["id"], "assign": [s.ids.carol] }]),
        )
        .await;
    assert_eq!(r.body["message"], "负责人须是这个文件夹的共享人或成员");
    let r = t.tool(&s.bob, "edit", json!({ "task": 1, "assign": ["alice"] })).await;
    assert_eq!(r.body["message"], "只有共享的文件夹里的事项能设负责人");
}

#[tokio::test]
async fn moving_into_a_shared_folder_cuts_links_and_renumbers_until_undone() {
    let s = shared().await;
    let t = &s.t;
    let r = t.ok(&s.bob, "add", json!({ "title": "选灯" })).await;
    assert!(text(&r).contains("#1 选灯"));
    t.ok(&s.bob, "add", json!({ "title": "买灯", "after": [1] })).await;

    let moved = t.ok(&s.bob, "move", json!({ "tasks": [2], "to": "@alice/装修" })).await;
    assert!(text(&moved).contains("删除依赖"), "{}", text(&moved));
    let snap = t.snapshot(&s.alice).await;
    let lamp = task(&snap, "买灯");
    assert_eq!((lamp["number"].as_i64(), lamp["folder_id"].as_str()), (Some(3), Some(s.home.as_str())));
    let bobs = t.snapshot(&s.bob).await;
    assert_eq!(bobs["dependencies"], json!([]));

    t.ok(&s.bob, "undo", json!({ "batch": batch(&moved) })).await;
    let bobs = t.snapshot(&s.bob).await;
    let lamp = task(&bobs, "买灯");
    assert_eq!((lamp["number"].as_i64(), lamp["folder_id"].clone()), (Some(2), Value::Null));
    assert_eq!(bobs["dependencies"].as_array().unwrap().len(), 1);
    assert!(!titles(&t.snapshot(&s.alice).await).contains(&"买灯"));
}

#[tokio::test]
async fn moving_out_drops_assignees_and_hides_the_task_from_members() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.alice, "edit", json!({ "task": 1, "assign": ["bob"] })).await;
    let since = seq(t, &s.bob).await;
    t.ok(&s.alice, "move", json!({ "tasks": [1], "to": "/私人" })).await;
    let measure = task(&t.snapshot(&s.alice).await, "量尺寸").clone();
    assert_eq!((measure["folder_id"].as_str(), measure["assignees"].clone()), (Some(s.private.as_str()), json!([])));
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&s.bob), None).await.body;
    let effects = changes["changes"][0]["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!((effects[0]["before"]["title"].as_str(), effects[0]["after"].clone()), (Some("量尺寸"), Value::Null), "to Bob it is gone");
    assert_eq!(titles(&t.snapshot(&s.bob).await), Vec::<&str>::new());
}

#[tokio::test]
async fn the_record_of_times_a_task_was_done_goes_where_it_goes() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.alice, "add", json!({ "title": "浇花", "folder": "/私人", "due": "2026-11-10", "repeat": "1d" })).await;
    t.ok(&s.alice, "done", json!({ "tasks": [3], "today": "2026-11-11" })).await;
    let records = t.snapshot(&s.alice).await["completions"].clone();
    assert_eq!(records[0]["due"], "2026-11-10");
    // What Bob gets of each move: the task coming or going, and its record with it.
    let moved = async |to: &str| {
        let since = seq(t, &s.bob).await;
        t.ok(&s.alice, "move", json!({ "tasks": [3], "to": to })).await;
        let changes = t.call("GET", &format!("/changes?since={since}"), Some(&s.bob), None).await.body;
        let effects = changes["changes"][0]["effects"].as_array().unwrap().clone();
        effects.iter().map(|e| (e["kind"].clone(), e["before"].is_null(), e["after"].is_null())).collect::<Vec<_>>()
    };
    assert_eq!(moved("/装修").await, [(json!("task"), true, false), (json!("completion"), true, false)]);
    assert_eq!(t.snapshot(&s.bob).await["completions"], records);
    assert_eq!(moved("/私人").await, [(json!("task"), false, true), (json!("completion"), false, true)]);
    assert_eq!(t.snapshot(&s.bob).await["completions"], json!([]));
}

#[tokio::test]
async fn a_shared_folder_moves_and_is_renamed_only_by_its_owner() {
    let s = shared().await;
    let t = &s.t;
    let r = t.tool(&s.bob, "rename", json!({ "folder": "@alice/装修", "name": "新家" })).await;
    assert_eq!(r.body["message"], "只有共享人能改名、移动或删除共享的文件夹");
    // Its subfolders are everyone's to change.
    t.ok(&s.bob, "rename", json!({ "folder": "@alice/装修/水电", "name": "水电改造" })).await;
    // Bob sees it at the top of his tree, wherever Alice keeps it.
    let since = seq(t, &s.bob).await;
    t.ok(&s.alice, "move", json!({ "folders": ["/装修"], "to": "/私人" })).await;
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&s.bob), None).await.body;
    assert_eq!(changes["changes"], json!([]), "a move Bob can't see the ends of reaches him as nothing");
    assert!(t.snapshot(&s.bob).await["folders"].as_array().unwrap().iter().all(|f| f["parent_id"].is_null() || f["name"] == "水电改造"));
    let r = t.tool(&s.bob, "mkdir", json!({ "path": "@alice/装修/软装" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn members_leave_or_are_removed_with_what_they_had_there() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.alice, "share", json!({ "folder": "/装修", "users": ["carol"] })).await;
    t.ok(&s.alice, "edit", json!({ "task": 1, "assign": ["bob", "carol"] })).await;
    let (organize, tidy) = (id(), id());
    let local_time = "2026-11-07 10:00 星期六";
    t.mutate(
        &s.bob,
        json!([
            { "op": "ai_job.create", "id": organize, "text": "买插座", "local_time": local_time, "folder_id": s.home },
            { "op": "ai_job.create", "id": tidy, "kind": "tidy", "text": "整理一下", "local_time": local_time, "folder_id": s.home },
        ]),
    )
    .await;
    let since = seq(t, &s.bob).await;

    let r = t.ok(&s.alice, "unshare", json!({ "folder": "/装修", "users": ["bob"] })).await;
    assert!(text(&r).contains("移除成员"), "{}", text(&r));
    let bobs = t.snapshot(&s.bob).await;
    assert_eq!((bobs["folders"].clone(), bobs["shares"].clone()), (json!([]), json!([])));
    // What Bob asked AI to organize lands in his own root; the tidying of a folder he has lost goes.
    let jobs = bobs["ai_jobs"].as_array().unwrap();
    assert_eq!((jobs.len(), jobs[0]["id"].as_str(), jobs[0]["folder_id"].clone()), (1, Some(organize.as_str()), Value::Null));
    let r = t.call("GET", &format!("/changes?since={since}"), Some(&s.bob), None).await;
    assert_eq!(r.status, StatusCode::GONE);
    let alices = t.snapshot(&s.alice).await;
    assert_eq!(task(&alices, "量尺寸")["assignees"], json!([s.ids.carol]));
    assert_eq!(alices["shares"][0]["members"], json!([s.ids.carol]));

    t.ok(&s.carol, "unshare", json!({ "folder": "@alice/装修" })).await;
    let alices = t.snapshot(&s.alice).await;
    assert_eq!((alices["shares"].clone(), task(&alices, "量尺寸")["assignees"].clone()), (json!([]), json!([])));
    assert_eq!(t.snapshot(&s.carol).await["folders"], json!([]));
}

#[tokio::test]
async fn undo_takes_a_whole_batch_seen_by_someone_allowed_to_make_it() {
    let s = shared().await;
    let t = &s.t;
    // One batch of Bob's, partly in his own data.
    let both = t
        .mutate(
            &s.bob,
            json!([
                { "op": "task.create", "id": id(), "title": "私事" },
                { "op": "task.create", "id": id(), "title": "贴瓷砖", "folder_id": s.home },
            ]),
        )
        .await
        .body["seq"]
        .as_i64()
        .unwrap();
    let r = t.tool(&s.alice, "undo", json!({ "batch": both })).await;
    assert_eq!((r.status, r.body["message"].as_str()), (StatusCode::FORBIDDEN, Some("这个批次里有无权查看的改动，无法撤销")));

    let paint = t.ok(&s.bob, "add", json!({ "title": "刷墙", "folder": "@alice/装修" })).await;
    t.ok(&s.alice, "undo", json!({ "batch": batch(&paint) })).await;
    assert!(!titles(&t.snapshot(&s.bob).await).contains(&"刷墙"));

    // Membership is the owner's to undo: Bob can't take back Alice's sharing, nor his own leaving.
    let shared_at: i64 = t.ok(&s.alice, "log", json!({ "limit": 100 })).await["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["summary"].as_str().unwrap().contains("添加成员"))
        .unwrap()["batch"]
        .as_i64()
        .unwrap();
    let r = t.tool(&s.bob, "undo", json!({ "batch": shared_at })).await;
    assert_eq!((r.status, r.body["message"].as_str()), (StatusCode::FORBIDDEN, Some("只有共享人能添加或移除成员")));
    t.ok(&s.alice, "undo", json!({ "batch": shared_at })).await;
    assert_eq!(t.snapshot(&s.bob).await["folders"], json!([]));
}

#[tokio::test]
async fn a_new_name_reaches_those_sharing_folders_with_one() {
    let s = shared().await;
    let t = &s.t;
    let since = seq(t, &s.alice).await;
    let r = t.call("PATCH", "/me", Some(&s.bob), Some(json!({ "display_name": "Bob" }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&s.alice), None).await.body;
    assert_eq!(changes["changes"][0]["effects"][0]["after"]["display_name"], "Bob");
    let carols = t.call("GET", "/changes?since=0", Some(&s.carol), None).await.body;
    assert_eq!(carols["changes"], json!([]));
}

#[tokio::test]
async fn a_new_name_reaches_those_who_see_what_one_made() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.alice, "share", json!({ "folder": "/装修", "users": ["carol"] })).await;
    t.ok(&s.bob, "add", json!({ "title": "买电线", "folder": "@alice/装修" })).await;
    t.ok(&s.alice, "unshare", json!({ "folder": "/装修", "users": ["bob"] })).await;
    let since = seq(t, &s.carol).await;
    let r = t.call("PATCH", "/me", Some(&s.bob), Some(json!({ "display_name": "Bob" }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for who in [&s.alice, &s.carol] {
        let changes = t.call("GET", &format!("/changes?since={since}"), Some(who), None).await.body;
        assert_eq!(changes["changes"][0]["effects"][0]["after"]["display_name"], "Bob");
    }
}

#[tokio::test]
async fn sharing_checks_who_and_what() {
    let s = shared().await;
    let t = &s.t;
    for (who, args, message) in [
        (&s.bob, json!({ "folder": "@alice/装修", "users": ["carol"] }), "只有共享人能添加成员"),
        (&s.alice, json!({ "folder": "/装修", "users": ["bob"] }), "bob 已是成员"),
        (&s.alice, json!({ "folder": "/装修", "users": ["dave"] }), "没有用户“dave”"),
        (&s.alice, json!({ "folder": "/装修", "users": ["alice"] }), "不能共享给自己"),
        (&s.alice, json!({ "folder": "/装修/水电", "users": ["carol"] }), "共享的文件夹里的文件夹不能再共享"),
    ] {
        let r = t.tool(who, "share", args).await;
        assert_eq!(r.body["message"], message);
    }
}

#[tokio::test]
async fn a_folder_moved_in_is_the_owners_even_where_numbers_coincide() {
    let s = shared().await;
    let t = &s.t;
    // Alice's next number is 3, as is that of Bob's third task.
    t.ok(&s.bob, "add", json!({ "title": "一" })).await;
    t.ok(&s.bob, "add", json!({ "title": "二" })).await;
    t.ok(&s.bob, "add", json!({ "title": "选灯", "folder": "/灯具" })).await;
    let moved = t.ok(&s.bob, "move", json!({ "folders": ["/灯具"], "to": "@alice/装修" })).await;
    let lamp = task(&t.snapshot(&s.alice).await, "选灯").clone();
    assert_eq!(lamp["number"], 3);
    // Undone and done again, it is #3 of Bob's and then of Alice's again.
    let undone = t.ok(&s.bob, "undo", json!({ "batch": batch(&moved) })).await;
    assert_eq!(task(&t.snapshot(&s.bob).await, "选灯")["number"], 3);
    assert!(t.ok(&s.bob, "show", json!({ "task": 3 })).await["text"].as_str().unwrap().contains("选灯"));
    t.ok(&s.bob, "undo", json!({ "batch": batch(&undone) })).await;
    assert_eq!(task(&t.snapshot(&s.alice).await, "选灯")["number"], 3);

    t.ok(&s.alice, "unshare", json!({ "folder": "/装修" })).await;
    assert!(!titles(&t.snapshot(&s.bob).await).contains(&"选灯"));
    let r = t.mutate(&s.bob, json!([{ "op": "task.update", "id": lamp["id"], "title": "改掉" }])).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
}

#[tokio::test]
async fn deleting_a_folder_with_a_shared_one_inside_tells_its_members_and_undoes_whole() {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    t.ok(&alice, "add", json!({ "title": "量尺寸", "folder": "/外层/装修" })).await;
    t.ok(&alice, "share", json!({ "folder": "/外层/装修", "users": ["bob"] })).await;
    t.ok(&alice, "edit", json!({ "task": 1, "assign": ["bob"] })).await;
    let since = seq(&t, &bob).await;

    let deleted = t.ok(&alice, "delete", json!({ "folders": ["/外层"] })).await;
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&bob), None).await.body;
    assert_eq!(changes["changes"].as_array().unwrap().len(), 1, "{changes}");
    assert_eq!((t.snapshot(&bob).await["folders"].clone(), t.snapshot(&bob).await["shares"].clone()), (json!([]), json!([])));

    t.ok(&alice, "undo", json!({ "batch": batch(&deleted) })).await;
    let bobs = t.snapshot(&bob).await;
    assert_eq!(bobs["shares"].as_array().unwrap().len(), 1);
    assert_eq!(task(&bobs, "量尺寸")["assignees"], json!([t.me(&bob).await]), "the task is Bob's to do again");
}

#[tokio::test]
async fn graphs_tell_apart_the_numbers_of_different_owners() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.bob, "add", json!({ "title": "甲" })).await;
    t.ok(&s.bob, "add", json!({ "title": "乙", "after": [1] })).await;
    t.ok(&s.bob, "add", json!({ "title": "丙", "folder": "@alice/装修", "after": ["@alice#1"] })).await;

    let graph = t.ok(&s.bob, "graph", json!({})).await;
    let mut edges: Vec<String> =
        graph["data"]["edges"].as_array().unwrap().iter().map(|e| format!("{} → {}", e["before"], e["after"])).collect();
    edges.sort();
    assert_eq!(edges, ["\"@alice#1\" → \"@alice#3\"", "1 → 2"]);
    assert!(text(&graph).contains("@alice#3 [可做] 丙") || text(&graph).contains("← @alice#1"), "{}", text(&graph));
    let mermaid = t.ok(&s.bob, "graph", json!({ "mermaid": true })).await;
    assert_eq!(text(&mermaid).lines().filter(|l| l.contains("[\"")).count(), 4, "{}", text(&mermaid));
    assert_eq!(text(&mermaid).lines().filter(|l| l.contains("-->")).count(), 2);
}

#[tokio::test]
async fn changes_in_a_shared_folder_read_as_its_owners_also_once_it_is_gone() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.bob, "add", json!({ "title": "自己的" })).await;
    t.ok(&s.bob, "add", json!({ "title": "接电线", "folder": "@alice/装修/水电" })).await;
    let deleted = t.ok(&s.bob, "delete", json!({ "folders": ["@alice/装修/水电"] })).await;
    assert!(
        text(&deleted).contains("删除文件夹 @alice/装修/水电") && text(&deleted).contains("删除 @alice#3 接电线"),
        "{}",
        text(&deleted)
    );
    let gone = deleted["data"]["changes"].as_array().unwrap().iter().find(|c| c["kind"] == "task_deleted").unwrap();
    assert_eq!((gone["task"].as_i64(), gone["owner"].as_str()), (Some(3), Some("alice")));
    assert!(t.ok(&s.bob, "log", json!({})).await["data"][0]["summary"].as_str().unwrap().contains("@alice#3"));

    // Alice deletes it all: Bob no longer shares anything with her, and still reads it as hers.
    t.ok(&s.alice, "delete", json!({ "folders": ["/装修"] })).await;
    let log = t.ok(&s.bob, "log", json!({})).await;
    let summary = log["data"][0]["summary"].as_str().unwrap();
    assert!(summary.contains("@alice/装修") && summary.contains("@alice#1"), "{summary}");
    assert!(text(&log).lines().next().unwrap().ends_with("· Alice"), "{}", text(&log));
    assert_eq!(log["data"][0]["user"], "alice");
}

#[tokio::test]
async fn undoing_lends_no_numbers_to_what_follows_it_in_the_same_change() {
    let s = shared().await;
    let t = &s.t;
    // Bob's #3 is the number Alice gives out next.
    t.ok(&s.bob, "add", json!({ "title": "一" })).await;
    t.ok(&s.bob, "add", json!({ "title": "二" })).await;
    t.ok(&s.bob, "add", json!({ "title": "选灯", "folder": "/灯具" })).await;
    let bobs = t.snapshot(&s.bob).await;
    let lamps = bobs["folders"].as_array().unwrap().iter().find(|f| f["name"] == "灯具").unwrap()["id"].clone();
    let renamed = id();
    let ops = json!([{ "op": "task.update", "id": task(&bobs, "选灯")["id"], "title": "选灯具" }]);
    t.call("POST", "/mutations", Some(&s.bob), Some(json!({ "id": renamed, "ops": ops }))).await;
    let ops = json!([{ "op": "undo", "mutation": renamed }, { "op": "folder.update", "id": lamps, "parent_id": s.home }]);
    let r = t.mutate(&s.bob, ops).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(task(&t.snapshot(&s.alice).await, "选灯")["number"], 3);
    let added = t.ok(&s.alice, "add", json!({ "title": "买灯泡" })).await;
    assert!(text(&added).contains("#4 买灯泡"), "{}", text(&added));
}

#[tokio::test]
async fn a_task_brought_back_among_someone_elses_takes_a_number_of_theirs() {
    let s = shared().await;
    let t = &s.t;
    for title in ["一", "二", "三"] {
        t.ok(&s.bob, "add", json!({ "title": title })).await;
    }
    t.ok(&s.bob, "add", json!({ "title": "选灯", "folder": "/灯具" })).await;
    let deleted = t.ok(&s.bob, "delete", json!({ "tasks": [4] })).await;
    t.ok(&s.bob, "move", json!({ "folders": ["/灯具"], "to": "@alice/装修" })).await;
    // Its folder is Alice's now, whose next number is 3: #4 is not one of hers to give.
    t.ok(&s.bob, "undo", json!({ "batch": batch(&deleted) })).await;
    assert_eq!(task(&t.snapshot(&s.alice).await, "选灯")["number"], 3);
    let added = t.ok(&s.alice, "add", json!({ "title": "买灯泡" })).await;
    assert!(text(&added).contains("#4 买灯泡"), "{}", text(&added));
}

#[tokio::test]
async fn undoing_two_moves_at_once_gives_back_the_numbers_each_took() {
    let s = shared().await;
    let t = &s.t;
    for title in ["一", "二", "三", "四"] {
        t.ok(&s.bob, "add", json!({ "title": title })).await;
    }
    t.ok(&s.bob, "add", json!({ "title": "选灯", "folder": "/灯具" })).await;
    let lamps = t.snapshot(&s.bob).await["folders"].as_array().unwrap().iter().find(|f| f["name"] == "灯具").unwrap()["id"].clone();
    let (moved_in, moved_out) = (id(), id());
    for (m, parent) in [(&moved_in, json!(s.home)), (&moved_out, Value::Null)] {
        let ops = json!([{ "op": "folder.update", "id": lamps, "parent_id": parent }]);
        let r = t.call("POST", "/mutations", Some(&s.bob), Some(json!({ "id": m, "ops": ops }))).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    }
    // Bob's #5, Alice's #3, Bob's #6; undone back to front: Alice's #3, Bob's #5.
    assert_eq!(task(&t.snapshot(&s.bob).await, "选灯")["number"], 6);
    let r = t.mutate(&s.bob, json!([{ "op": "undo", "mutation": moved_out }, { "op": "undo", "mutation": moved_in }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(task(&t.snapshot(&s.bob).await, "选灯")["number"], 5);
}

#[tokio::test]
async fn a_task_tells_who_made_it_also_once_they_have_left() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.bob, "add", json!({ "title": "买电线", "folder": "@alice/装修" })).await;
    t.ok(&s.alice, "unshare", json!({ "folder": "/装修" })).await;
    let shown = t.ok(&s.alice, "show", json!({ "task": 3 })).await;
    assert!(text(&shown).contains("创建：刚刚，bob"), "{}", text(&shown));
    assert_eq!((shown["data"]["created_by"].as_str(), shown["data"]["updated_by"].as_str()), (Some("bob"), Some("bob")));
}

#[tokio::test]
async fn whoever_made_a_task_back_in_view_is_met_with_it() {
    let s = shared().await;
    let t = &s.t;
    t.ok(&s.alice, "share", json!({ "folder": "/装修", "users": ["carol"] })).await;
    t.ok(&s.bob, "add", json!({ "title": "买电线", "folder": "@alice/装修" })).await;
    let deleted = t.ok(&s.alice, "delete", json!({ "tasks": [3] })).await;
    t.ok(&s.alice, "unshare", json!({ "folder": "/装修", "users": ["bob"] })).await;
    let usernames = |snap: &Value| -> Vec<String> {
        snap["people"].as_array().unwrap().iter().map(|p| p["username"].as_str().unwrap().to_owned()).collect()
    };
    assert!(!usernames(&t.snapshot(&s.carol).await).contains(&"bob".to_owned()));

    let since = seq(t, &s.carol).await;
    t.ok(&s.alice, "undo", json!({ "batch": batch(&deleted) })).await;
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&s.carol), None).await.body;
    let effects = changes["changes"][0]["effects"].as_array().unwrap();
    assert!(effects.iter().any(|e| e["kind"] == "person" && e["after"]["username"] == "bob"), "{effects:?}");
    assert!(usernames(&t.snapshot(&s.carol).await).contains(&"bob".to_owned()));
}

#[tokio::test]
async fn copies_keep_assignees_only_inside_the_same_shared_folder() {
    let Shared { t, alice, bob, ids, home, .. } = shared().await;
    t.ok(&alice, "edit", json!({ "task": 1, "assign": ["bob"] })).await;
    t.ok(&alice, "add", json!({ "title": "布线", "folder": "/装修/水电", "assign": ["bob"] })).await;

    // Shared with Bob, the folder copies to his root, his own: no one is assigned there.
    t.ok(&bob, "copy", json!({ "folder": "@alice/装修" })).await;
    let snap = t.snapshot(&bob).await;
    let mine: Vec<&Value> = snap["tasks"].as_array().unwrap().iter().filter(|x| x["title"] == "量尺寸" && x["folder_id"] != home).collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["assignees"], json!([]));
    assert!(
        t.snapshot(&bob).await["folders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "装修" && f["parent_id"].is_null() && f["id"] != home)
    );

    // Copied within the shared folder, it stays Alice's, and Bob's to do.
    t.ok(&bob, "copy", json!({ "folder": "@alice/装修/水电" })).await;
    let snap = t.snapshot(&alice).await;
    let wiring: Vec<&Value> = snap["tasks"].as_array().unwrap().iter().filter(|x| x["title"] == "布线").collect();
    assert_eq!(wiring.len(), 2);
    assert!(wiring.iter().all(|x| x["assignees"] == json!([ids.bob])), "{wiring:?}");

    // Nor in a private folder holding a shared one.
    t.ok(&alice, "mkdir", json!({ "path": "/项目" })).await;
    t.ok(&alice, "move", json!({ "folders": ["/装修"], "to": "/项目" })).await;
    t.ok(&alice, "copy", json!({ "folder": "/项目" })).await;
    let snap = t.snapshot(&alice).await;
    let copy = snap["folders"].as_array().unwrap().iter().find(|f| f["name"] == "项目 副本").unwrap();
    let inside = snap["folders"].as_array().unwrap().iter().find(|f| f["parent_id"] == copy["id"]).unwrap();
    let measure = snap["tasks"].as_array().unwrap().iter().find(|x| x["folder_id"] == inside["id"]).unwrap();
    assert_eq!(measure["assignees"], json!([]));
    t.ok(&alice, "move", json!({ "folders": ["/项目/装修"], "to": "/" })).await;

    // Alice's copy of the shared folder itself is hers alone.
    t.ok(&alice, "copy", json!({ "folder": "/装修" })).await;
    assert!(t.snapshot(&bob).await["folders"].as_array().unwrap().iter().all(|f| f["name"] != "装修 副本"));
}

#[tokio::test]
async fn a_folder_moved_into_a_shared_one_keeps_the_due_dates_its_prerequisites_gave() {
    let s = shared().await;
    let (t, alice) = (&s.t, &s.alice);
    let post = |today: &str, ops: Value| t.call("POST", "/mutations", Some(alice), Some(json!({ "id": id(), "today": today, "ops": ops })));
    let (flowers, water, log, outside, plan) = (id(), id(), id(), id(), id());
    let daily = json!({ "date": "2026-11-20", "repeat": { "every": 1, "unit": "day", "from": "due" } });
    let r = post(
        "2026-11-20",
        json!([
            { "op": "folder.create", "id": flowers, "name": "养花" },
            { "op": "task.create", "id": water, "title": "浇花", "folder_id": flowers, "due": daily },
            { "op": "task.create", "id": log, "title": "记下长势", "folder_id": flowers, "due_after": { "days": 1 } },
            { "op": "dep.add", "before": water, "after": log },
            { "op": "task.create", "id": outside, "title": "买花盆", "folder_id": s.private },
            { "op": "task.create", "id": plan, "title": "换盆", "folder_id": flowers, "due_after": { "days": 2 } },
            { "op": "dep.add", "before": outside, "after": plan },
            { "op": "task.update", "id": water, "done": true, "today": "2026-11-20" },
        ]),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // Watering has come round again by the time the folder moves; its dependency moves along.
    let r = post("2026-11-23", json!([{ "op": "folder.update", "id": flowers, "parent_id": s.home }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let snap = t.snapshot(alice).await;
    assert_eq!(task(&snap, "记下长势")["due"]["date"], "2026-11-21");
    // The one whose only prerequisite stayed behind has none left.
    assert_eq!(task(&snap, "换盆")["due"]["date"], "2026-11-25");
}
