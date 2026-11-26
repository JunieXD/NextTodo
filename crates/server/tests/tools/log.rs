//! The log of changes, read as each batch was, and undone.

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::{self, TestApp};
use crate::text;

#[tokio::test]
async fn the_log_reads_each_batch_with_the_folders_of_its_time() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "mkdir", json!({ "path": "/A" })).await;
    t.ok(&s, "mkdir", json!({ "path": "/B" })).await;
    t.ok(&s, "move", json!({ "folders": ["/A"], "to": "/B" })).await;
    t.ok(&s, "move", json!({ "folders": ["/B/A"], "to": "/" })).await;
    t.ok(&s, "move", json!({ "folders": ["/B"], "to": "/A" })).await;
    t.ok(&s, "delete", json!({ "folders": ["/A"] })).await;
    let log = t.ok(&s, "log", json!({})).await["data"].clone();
    let summaries: Vec<&str> = log.as_array().unwrap().iter().map(|b| b["summary"].as_str().unwrap()).collect();
    assert_eq!(
        summaries,
        vec![
            "删除 2 个文件夹：/A/B、/A",
            "移动文件夹 /B → /A/B",
            "移动文件夹 /B/A → /A",
            "移动文件夹 /A → /B/A",
            "新建文件夹 /B",
            "新建文件夹 /A"
        ]
    );
}

#[tokio::test]
async fn changes_inside_one_batch_read_with_the_folders_of_that_moment() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (a, b) = (common::id(), common::id());
    t.mutate(
        &s,
        json!([
            { "op": "folder.create", "id": a, "name": "A" },
            { "op": "folder.create", "id": b, "name": "B", "parent_id": a },
            { "op": "folder.delete", "id": a },
        ]),
    )
    .await;
    let log = t.ok(&s, "log", json!({})).await["data"].clone();
    assert_eq!(log[0]["summary"], "新建 2 个文件夹：/A、/A/B；删除 2 个文件夹：/A/B、/A");
}

#[tokio::test]
async fn the_log_gives_one_batch_in_full() {
    let t = TestApp::new();
    let s = t.admin().await;
    for title in ["一", "二", "三", "四"] {
        t.ok(&s, "add", json!({ "title": title })).await;
    }
    let done = t.ok(&s, "done", json!({ "tasks": [1, 2, 3, 4] })).await["data"]["batch"].clone();
    assert_eq!(t.ok(&s, "log", json!({ "limit": 1 })).await["data"][0]["changes"], json!(["完成 4 项：#1 一、#2 二、#3 三 等"]));
    // Whatever else is asked.
    let one = t.ok(&s, "log", json!({ "batch": done, "limit": 0, "deleted": true })).await["data"].clone();
    assert_eq!((&one[0]["changes"], &one[0]["undoes"]), (&json!(["完成 4 项：#1 一、#2 二、#3 三、#4 四"]), &Value::Null));
    let undo = t.ok(&s, "undo", json!({ "batch": done })).await["data"]["batch"].clone();
    let one = t.ok(&s, "log", json!({ "batch": undo })).await;
    assert_eq!((&one["data"][0]["undoes"], one["data"][0]["changes"].as_array().unwrap().len()), (&done, 1));
    // In full, each in the order done: undoing, the latest first.
    assert!(
        text(&one).ends_with("\n  撤销批次 5\n  重新打开 #4 四\n  重新打开 #3 三\n  重新打开 #2 二\n  重新打开 #1 一"),
        "{}",
        text(&one)
    );
    assert_eq!(t.ok(&s, "log", json!({ "batch": 999 })).await["data"], json!([]));
}

#[tokio::test]
async fn one_batch_in_full_says_what_each_field_was_and_became() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "mkdir", json!({ "path": "/工作" })).await;
    t.ok(&s, "mkdir", json!({ "path": "/生活" })).await;
    let batch = |r: Value| r["data"]["batch"].clone();
    let add = json!({ "title": "报销", "folder": "/工作", "due": "2026-11-20 18:00", "remind": 60, "description": "发票\r\n收据" });
    let added = batch(t.ok(&s, "add", add).await);
    let field =
        |field: &str, name: &str, before: Value, after: Value| json!({ "field": field, "name": name, "before": before, "after": after });
    // What it was added with, its title aside, which the line has.
    let log = t.ok(&s, "log", json!({ "batch": added })).await;
    assert_eq!(
        log["data"][0]["items"],
        json!([{ "text": "添加 #1 报销", "change": "created", "fields": [
            field("description", "描述", Value::Null, json!("发票\r\n收据")),
            field("folder", "所在文件夹", Value::Null, json!("/工作")),
            field("due", "截止", Value::Null, json!("2026-11-20 18:00")),
            field("reminders", "提醒", Value::Null, json!("提前 1 小时")),
        ] }])
    );
    assert!(
        text(&log)
            .ends_with("\n  添加 #1 报销\n    描述：发票 收据\n    所在文件夹：/工作\n    截止：2026-11-20 18:00\n    提醒：提前 1 小时"),
        "{}",
        text(&log)
    );
    let edited = batch(t.ok(&s, "edit", json!({ "task": 1, "title": "整理报销单", "due": "2026-11-22", "remind": null })).await);
    let log = t.ok(&s, "log", json!({ "batch": edited })).await;
    assert!(
        text(&log).ends_with(
            "\n  修改 #1 整理报销单\n    标题：报销 → 整理报销单\n    截止：2026-11-20 18:00 → 2026-11-22\n    提醒：提前 1 小时 → 无"
        ),
        "{}",
        text(&log)
    );
    let moved = batch(t.ok(&s, "move", json!({ "tasks": [1], "to": "/生活" })).await);
    let log = t.ok(&s, "log", json!({ "batch": moved })).await;
    assert_eq!(log["data"][0]["items"][0]["fields"], json!([field("folder", "所在文件夹", json!("/工作"), json!("/生活"))]));
    let renamed = batch(t.ok(&s, "rename", json!({ "folder": "/工作", "name": "公司" })).await);
    assert!(text(&t.ok(&s, "log", json!({ "batch": renamed })).await).ends_with("\n  文件夹改名 /公司\n    名称：工作 → 公司"));
    // Folders as they were then, renamed and deleted since.
    t.ok(&s, "delete", json!({ "folders": ["/公司"] })).await;
    let log = t.ok(&s, "log", json!({ "batch": moved })).await;
    assert_eq!(log["data"][0]["items"][0]["fields"], json!([field("folder", "所在文件夹", json!("/工作"), json!("/生活"))]));
    // Done, a repeating one moves on.
    t.ok(&s, "add", json!({ "title": "浇花", "due": "2026-11-10", "repeat": "1d" })).await;
    let done = batch(t.ok(&s, "done", json!({ "tasks": [2], "today": "2026-11-10" })).await);
    let log = t.ok(&s, "log", json!({ "batch": done })).await;
    assert!(text(&log).ends_with("\n  顺延 #2 浇花\n    截止：2026-11-10 → 2026-11-11\n    完成次数：无 → 1 次"), "{}", text(&log));
    // A title in full, however long.
    let long = "把十一月出差的机票、火车票、酒店水单和餐饮发票全部整理好再交给财务";
    let added = batch(t.ok(&s, "add", json!({ "title": long })).await);
    assert_eq!(t.ok(&s, "log", json!({ "batch": added })).await["data"][0]["items"][0]["text"], format!("添加 #3 {long}"));
    // What was deleted, which undoing brings back.
    let deleted = batch(t.ok(&s, "delete", json!({ "tasks": [1] })).await);
    let log = t.ok(&s, "log", json!({ "batch": deleted })).await;
    assert_eq!(log["data"][0]["items"][0]["change"], "deleted");
    assert!(
        text(&log).ends_with("\n  删除 #1 整理报销单\n    描述：发票 收据\n    所在文件夹：/生活\n    截止：2026-11-22"),
        "{}",
        text(&log)
    );
    // Of what has a line alone, whether it was added or deleted too.
    t.user(&s, "bob").await;
    t.ok(&s, "share", json!({ "folder": "/生活", "users": ["bob"] })).await;
    let shared = t.ok(&s, "log", json!({ "limit": 1 })).await["data"][0]["batch"].clone();
    let item = &t.ok(&s, "log", json!({ "batch": shared })).await["data"][0]["items"][0];
    assert_eq!((&item["text"], &item["change"]), (&json!("添加成员 bob（/生活）"), &json!("created")));
    // Only the one batch asked for, in full.
    assert_eq!(t.ok(&s, "log", json!({ "limit": 1 })).await["data"][0].get("items"), None);
}

#[tokio::test]
async fn a_batch_read_after_its_folders_were_turned_inside_out_and_one_deleted_reads_as_it_was() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "mkdir", json!({ "path": "/外层/内层" })).await;
    t.ok(&s, "add", json!({ "title": "报销", "folder": "/外层/内层" })).await;
    let snap = t.snapshot(&s).await;
    let inner = snap["folders"].as_array().unwrap().iter().find(|f| f["name"] == "内层").unwrap()["id"].clone();
    let ops = json!([
        { "op": "task.update", "id": snap["tasks"][0]["id"], "title": "整理报销单" },
        { "op": "folder.update", "id": inner, "parent_id": null },
    ]);
    let r = t.mutate(&s, ops).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let batch = t.ok(&s, "log", json!({ "limit": 1 })).await["data"][0]["batch"].clone();
    // The one it was in, in it now, and deleted.
    t.ok(&s, "move", json!({ "folders": ["/外层"], "to": "/内层" })).await;
    t.ok(&s, "delete", json!({ "folders": ["/内层/外层"] })).await;
    let items = t.ok(&s, "log", json!({ "batch": batch })).await["data"][0]["items"].clone();
    let texts: Vec<_> = items.as_array().unwrap().iter().map(|i| i["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["修改 #1 整理报销单", "移动文件夹 /内层"]);
    assert_eq!(items[1]["fields"][0], json!({ "field": "parent", "name": "位置", "before": "/外层", "after": "/" }));
}

#[tokio::test]
async fn a_batch_from_before_a_member_left_and_came_back_reads_without_what_they_missed() {
    let t = TestApp::new();
    let alice = t.admin().await;
    let bob = t.user(&alice, "bob").await;
    t.ok(&alice, "mkdir", json!({ "path": "/共享/外层/内层" })).await;
    t.ok(&alice, "add", json!({ "title": "报销", "folder": "/共享/外层/内层" })).await;
    t.ok(&alice, "share", json!({ "folder": "/共享", "users": ["bob"] })).await;
    let snap = t.snapshot(&alice).await;
    let folder = |name: &str| snap["folders"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap()["id"].clone();
    let ops = json!([
        { "op": "task.update", "id": snap["tasks"][0]["id"], "title": "整理报销单" },
        { "op": "folder.update", "id": folder("内层"), "parent_id": folder("共享") },
    ]);
    let r = t.mutate(&alice, ops).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let batch = t.ok(&alice, "log", json!({ "limit": 1 })).await["data"][0]["batch"].clone();
    // Turned inside out while bob is out: what he reads next goes back across what he never saw.
    t.ok(&alice, "unshare", json!({ "folder": "/共享", "users": ["bob"] })).await;
    t.ok(&alice, "move", json!({ "folders": ["/共享/外层"], "to": "/共享/内层" })).await;
    t.ok(&alice, "share", json!({ "folder": "/共享", "users": ["bob"] })).await;
    // Where the task was is as far as he can tell; the folder he saw moved is as it was.
    let items = t.ok(&bob, "log", json!({ "batch": batch })).await["data"][0]["items"].clone();
    let texts: Vec<_> = items.as_array().unwrap().iter().map(|i| i["text"].as_str().unwrap()).collect();
    assert_eq!((texts.len(), texts[0].ends_with("整理报销单"), texts[1]), (2, true, "移动文件夹 @alice/共享/内层"));
}

#[tokio::test]
async fn the_log_pages_back_finds_deletions_and_says_what_is_undone() {
    let t = TestApp::new();
    let s = t.admin().await;
    for title in ["甲", "乙"] {
        t.ok(&s, "add", json!({ "title": title })).await;
    }
    t.ok(&s, "delete", json!({ "tasks": [2] })).await;
    let log = |args: Value| {
        let t = &t;
        let s = s.clone();
        async move {
            t.ok(&s, "log", args).await["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| (b["batch"].as_i64().unwrap(), b["undone"].as_bool().unwrap()))
                .collect::<Vec<_>>()
        }
    };
    let all = log(json!({})).await;
    let deleted = all[0].0;
    t.ok(&s, "undo", json!({ "batch": deleted })).await;
    let restored = log(json!({ "limit": 1 })).await[0].0;
    // Undone, by a change not undone itself.
    assert_eq!(log(json!({ "limit": 2 })).await, [(restored, false), (deleted, true)]);
    // Undone again, it changes nothing and undoes nothing.
    t.ok(&s, "undo", json!({ "batch": deleted })).await;
    t.ok(&s, "undo", json!({ "batch": restored })).await;
    let latest = log(json!({ "limit": 4 })).await;
    assert_eq!(latest[2..], [(restored, true), (deleted, false)]);
    // Those that delete something: the first deletion, and undoing its undoing.
    assert_eq!(log(json!({ "deleted": true })).await, [latest[0], (deleted, false)]);
    // Paging back: those before a batch.
    assert_eq!(log(json!({ "before": deleted })).await, all[1..]);
    assert!(text(&t.ok(&s, "log", json!({ "limit": 3 })).await).contains("已撤销"));

    // Both undone by one batch.
    let (first, second) = (common::id(), common::id());
    for (title, id) in [("甲一", &first), ("甲二", &second)] {
        t.ok(&s, "edit", json!({ "task": 1, "title": title, "request_id": id })).await;
    }
    let r = t.mutate(&s, json!([{ "op": "undo", "mutation": second }, { "op": "undo", "mutation": first }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(log(json!({ "limit": 3 })).await.iter().map(|b| b.1).collect::<Vec<_>>(), [false, true, true]);
}

#[tokio::test]
async fn a_change_undone_twice_reads_undone_as_things_are() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "甲" })).await;
    let batch = |r: Value| r["data"]["batch"].clone();
    let deleted = batch(t.ok(&s, "delete", json!({ "tasks": [1] })).await);
    let restored = batch(t.ok(&s, "undo", json!({ "batch": deleted })).await);
    let again = batch(t.ok(&s, "undo", json!({ "batch": restored })).await);
    let back = batch(t.ok(&s, "undo", json!({ "batch": again })).await);
    // The restoring undone once more, not the last undo: deleted again, as the first and `again` did.
    let last = batch(t.ok(&s, "undo", json!({ "batch": restored })).await);
    let log = t.ok(&s, "log", json!({ "limit": 5 })).await["data"].clone();
    let read: Vec<_> = log.as_array().unwrap().iter().map(|b| (b["batch"].clone(), b["undone"].as_bool().unwrap())).collect();
    assert_eq!(read, [(last, false), (back, true), (again, false), (restored, true), (deleted, false)]);

    // One batch undoing two, itself undone, then one of them undone alone: the other is not undone.
    let (x, y) = (common::id(), common::id());
    let x_batch = batch(t.ok(&s, "add", json!({ "title": "乙", "request_id": x })).await);
    let y_batch = batch(t.ok(&s, "add", json!({ "title": "丙", "request_id": y })).await);
    let both = t.mutate(&s, json!([{ "op": "undo", "mutation": x }, { "op": "undo", "mutation": y }])).await.body["seq"].clone();
    let redone = batch(t.ok(&s, "undo", json!({ "batch": both })).await);
    let alone = batch(t.ok(&s, "undo", json!({ "batch": x_batch })).await);
    let log = t.ok(&s, "log", json!({ "limit": 5 })).await["data"].clone();
    let read: Vec<_> = log.as_array().unwrap().iter().map(|b| (b["batch"].clone(), b["undone"].as_bool().unwrap())).collect();
    assert_eq!(read, [(alone, false), (redone, false), (both, true), (y_batch, false), (x_batch, true)]);
    // The same, read a page at a time.
    for b in &read {
        let one = t.ok(&s, "log", json!({ "limit": 1, "before": b.0.as_i64().unwrap() + 1 })).await["data"][0]["undone"].clone();
        assert_eq!(one, json!(b.1), "{b:?}");
    }
}

#[tokio::test]
async fn a_start_set_or_undone_reads_as_an_edit() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "写周报" })).await;
    let start = json!([{ "kind": "task_edited", "task": 1, "fields": ["start"] }]);
    let mut batch = Value::Null;
    // The day, then only the time on it.
    for when in ["2026-12-04", "2026-12-04 09:30"] {
        let r = t.ok(&s, "edit", json!({ "task": 1, "start": when })).await;
        assert_eq!(r["data"]["changes"], start, "{when}");
        batch = r["data"]["batch"].clone();
    }
    assert_eq!(t.ok(&s, "undo", json!({ "batch": batch })).await["data"]["changes"], start);
}
