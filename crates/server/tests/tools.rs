mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::TestApp;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn numbers(list: &Value) -> Vec<i64> {
    list.as_array().unwrap().iter().map(|t| t["number"].as_i64().unwrap()).collect()
}

fn text(reply: &Value) -> &str {
    reply["text"].as_str().unwrap()
}

#[tokio::test]
async fn a_plan_builds_folders_tasks_and_dependencies_and_undoes_as_one_batch() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "部署", "folder": "/运维" })).await;
    let plan = json!({ "folder": "工作/网站改版", "tasks": [
        { "ref": "design", "title": "出设计稿", "due": "2026-11-20 18:00", "remind": 60 },
        { "ref": "api", "title": "写接口", "folder": "后端" },
        { "title": "联调", "after": ["design", "api", "#1"] },
    ] });

    let preview = t.ok(&s, "apply", json!({ "plan": plan, "dry_run": true })).await;
    assert_eq!(preview["data"]["batch"], Value::Null);
    assert!(text(&preview).starts_with("预览"), "{}", text(&preview));
    assert_eq!(t.snapshot(&s).await["tasks"].as_array().unwrap().len(), 1, "a preview saves nothing");

    let applied = t.ok(&s, "apply", json!({ "plan": plan })).await;
    assert!(text(&applied).contains("添加 3 项：#2 出设计稿、#3 写接口、#4 联调"), "{}", text(&applied));
    let graph = t.ok(&s, "graph", json!({ "folder": "/工作" })).await["data"].clone();
    assert_eq!(numbers(&graph["tasks"]), vec![1, 2, 3, 4], "the outside prerequisite is listed too");
    assert_eq!(graph["edges"], json!([{ "before": 1, "after": 4 }, { "before": 2, "after": 4 }, { "before": 3, "after": 4 }]));
    let joint = t.ok(&s, "show", json!({ "task": 4 })).await["data"].clone();
    assert_eq!((joint["status"].as_str(), joint["waiting_on"].as_i64()), (Some("waiting"), Some(3)));
    assert_eq!(joint["folder"], "/工作/网站改版");
    assert_eq!(t.ok(&s, "show", json!({ "task": 3 })).await["data"]["folder"], "/工作/网站改版/后端");
    let design = t.ok(&s, "show", json!({ "task": 2 })).await["data"].clone();
    assert_eq!(design["due"], json!({ "date": "2026-11-20", "time": "18:00", "reminders": [{ "before": 60 }] }));

    t.ok(&s, "undo", json!({ "batch": applied["data"]["batch"] })).await;
    let snap = t.snapshot(&s).await;
    assert_eq!(snap["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(snap["folders"].as_array().unwrap().len(), 1, "the folders the plan made are gone too");
}

#[tokio::test]
async fn a_read_says_which_change_it_is_as_of() {
    let t = TestApp::new();
    let s = t.admin().await;
    let before = t.ok(&s, "next", json!({})).await["seq"].as_i64().unwrap();
    let added = t.ok(&s, "add", json!({ "title": "交电费" })).await;
    assert_eq!(added.get("seq"), None, "a write gives its batch instead");
    let batch = added["data"]["batch"].as_i64().unwrap();
    assert!(before < batch);
    assert_eq!(t.ok(&s, "next", json!({})).await["seq"], batch);
}

#[tokio::test]
async fn completing_a_task_reports_what_it_unlocks() {
    let t = TestApp::new();
    let s = t.admin().await;
    let plan = json!({ "tasks": [
        { "ref": "a", "title": "写初稿" },
        { "ref": "b", "title": "改稿", "after": ["a"] },
        { "title": "交电费", "due": "2026-11-05" },
        { "title": "投稿", "after": ["b"] },
    ] });
    t.ok(&s, "apply", json!({ "plan": plan })).await;
    assert_eq!(numbers(&t.ok(&s, "next", json!({})).await["data"]["tasks"]), vec![3, 1], "tasks with a due date come first");

    let done = t.ok(&s, "done", json!({ "tasks": [1] })).await;
    assert_eq!(numbers(&done["data"]["unlocked"]), vec![2]);
    assert!(text(&done).contains("解锁 #2 改稿"), "{}", text(&done));
    assert_eq!(numbers(&t.ok(&s, "next", json!({})).await["data"]["tasks"]), vec![3, 2]);

    t.ok(&s, "reopen", json!({ "tasks": [1] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 2 })).await["data"]["status"], "waiting");
}

#[tokio::test]
async fn bad_plans_are_rejected_whole() {
    let t = TestApp::new();
    let s = t.admin().await;
    let apply = |tasks: Value| t.tool(&s, "apply", json!({ "plan": { "folder": "/新项目", "tasks": tasks } }));
    for (tasks, status, says) in [
        (json!([{ "title": "A", "after": ["nope"] }]), StatusCode::BAD_REQUEST, "nope"),
        (json!([{ "ref": "x", "title": "A" }, { "ref": "x", "title": "B" }]), StatusCode::BAD_REQUEST, "x"),
        (json!([{ "title": "A", "after": [99] }]), StatusCode::NOT_FOUND, "#99"),
        (
            json!([{ "ref": "x", "title": "甲", "after": ["y"] }, { "ref": "y", "title": "乙", "after": ["x"] }]),
            StatusCode::CONFLICT,
            "“甲”",
        ),
        (json!([{ "title": "A", "due": "2026-11-20", "remind": 10 }]), StatusCode::BAD_REQUEST, "具体的截止时间"),
        (json!([{ "title": "A", "due": "11月20日" }]), StatusCode::BAD_REQUEST, "日期"),
        (json!([{ "ref": "1", "title": "A" }, { "title": "B", "after": ["1"] }]), StatusCode::BAD_REQUEST, "编号"),
        (json!([{ "ref": "#1", "title": "A" }]), StatusCode::BAD_REQUEST, "编号"),
    ] {
        let r = apply(tasks).await;
        assert_eq!(r.status, status, "{}", r.body);
        assert!(r.body["message"].as_str().unwrap().contains(says), "{}", r.body);
    }
    let snap = t.snapshot(&s).await;
    assert_eq!((snap["folders"].clone(), snap["tasks"].clone()), (json!([]), json!([])), "not even the folder");
}

#[tokio::test]
async fn folders_are_named_by_path() {
    let t = TestApp::new();
    let s = t.admin().await;
    assert!(text(&t.ok(&s, "mkdir", json!({ "path": "/学习/毕业论文" })).await).contains("新建 2 个文件夹"));
    assert_eq!(t.tool(&s, "mkdir", json!({ "path": "学习/毕业论文" })).await.status, StatusCode::CONFLICT);
    t.ok(&s, "mkdir", json!({ "path": "/生活" })).await;
    assert_eq!(t.tool(&s, "rename", json!({ "folder": "/生活", "name": "学习" })).await.status, StatusCode::CONFLICT, "siblings differ");
    assert_eq!(t.tool(&s, "rename", json!({ "folder": "/生活", "name": "a/b" })).await.status, StatusCode::BAD_REQUEST);

    t.ok(&s, "rename", json!({ "folder": "/学习", "name": "课程" })).await;
    assert_eq!(t.tool(&s, "list", json!({ "folder": "/学习" })).await.status, StatusCode::NOT_FOUND);
    t.ok(&s, "add", json!({ "title": "写开题报告", "folder": "/课程/毕业论文" })).await;
    t.ok(&s, "add", json!({ "title": "买菜" })).await;
    t.ok(&s, "move", json!({ "tasks": [2], "folders": ["/课程/毕业论文"], "to": "/生活" })).await;
    let tree = t.ok(&s, "tree", json!({})).await["data"].clone();
    let paths: Vec<&str> = tree.as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["/", "/课程", "/生活", "/生活/毕业论文"]);
    // Counting those in subfolders too.
    let pending: Vec<u64> = tree.as_array().unwrap().iter().map(|f| f["pending"].as_u64().unwrap()).collect();
    assert_eq!(pending, vec![2, 0, 2, 1]);

    t.ok(&s, "delete", json!({ "tasks": [1], "folders": ["/生活", "/生活/毕业论文", "/生活"] })).await;
    let snap = t.snapshot(&s).await;
    assert_eq!((snap["folders"].as_array().unwrap().len(), snap["tasks"].clone()), (1, json!([])));
}

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
async fn a_tool_retried_with_the_id_of_an_app_change_gets_a_receipt() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (a, b) = (common::id(), common::id());
    t.mutate(&s, json!([{ "op": "folder.create", "id": a, "name": "A" }, { "op": "folder.create", "id": b, "name": "B", "parent_id": a }]))
        .await;
    let moved = common::id();
    let r = t
        .call("POST", "/mutations", Some(&s), Some(json!({ "id": moved, "ops": [{ "op": "folder.update", "id": b, "parent_id": null }] })))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    t.mutate(&s, json!([{ "op": "folder.update", "id": a, "parent_id": b }])).await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/tools/mkdir")
        .header(header::AUTHORIZATION, format!("Bearer {s}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("Idempotency-Key", &moved)
        .body(Body::from(json!({ "path": "/Z" }).to_string()))
        .unwrap();
    let bytes = t.app.clone().oneshot(req).await.unwrap().into_body().collect().await.unwrap().to_bytes();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["text"], "批次 2 已保存，本次未重复生效");
    assert!(t.snapshot(&s).await["folders"].as_array().unwrap().iter().all(|f| f["name"] != "Z"), "and nothing was made");
}

#[tokio::test]
async fn editing_a_task() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "交电费", "due": "2026-11-20" })).await;
    let cleared = t.ok(&s, "edit", json!({ "task": 1, "remind": null })).await;
    assert!(text(&cleared).starts_with("没有变化"), "there was no reminder to clear: {}", text(&cleared));
    assert_eq!(t.tool(&s, "edit", json!({ "task": 1, "remind": 10 })).await.status, StatusCode::BAD_REQUEST, "no time to remind before");
    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-20 18:00", "remind": 30 })).await;
    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-21 09:00" })).await;
    let due = t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"].clone();
    assert_eq!(
        due,
        json!({ "date": "2026-11-21", "time": "09:00", "reminders": [{ "before": 30 }] }),
        "moving the deadline keeps the reminder"
    );
    t.ok(&s, "edit", json!({ "task": 1, "remind": null })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"]["reminders"], Value::Null);
    // Several, the first to come first, also by the day; moved to a date alone, those before its time go.
    t.ok(&s, "edit", json!({ "task": 1, "remind": [0, "1d 20:00", "09:00"] })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(text(&shown).contains("截止：2026-11-21 09:00，提醒：前一天 20:00、截止时、当天 09:00"), "{}", text(&shown));
    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-22" })).await;
    let due = t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"].clone();
    assert_eq!(due, json!({ "date": "2026-11-22", "reminders": [{ "days": 1, "time": "20:00" }, { "days": 0, "time": "09:00" }] }));
    t.ok(&s, "edit", json!({ "task": 1, "remind": [] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"]["reminders"], Value::Null);
    for bad in [json!("1x 20:00"), json!("25:00"), json!(["31d 09:00"])] {
        assert_eq!(t.tool(&s, "edit", json!({ "task": 1, "remind": bad })).await.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(t.tool(&s, "edit", json!({ "task": 1 })).await.status, StatusCode::BAD_REQUEST, "nothing to change");
}

#[tokio::test]
async fn a_repeating_task_counts_its_completions_and_can_end_done() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "读《三体》", "due": "2026-11-10", "repeat": "1d" })).await;
    t.ok(&s, "add", json!({ "title": "写读后感", "after": [1] })).await;
    let first = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-10" })).await;
    let second = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-11" })).await;
    // Done again since, the first time can't be taken back alone: setting the count back would take
    // the second with it.
    let r = t.tool(&s, "undo", json!({ "batch": first["data"]["batch"] })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    // Nor once taken back and done again that day, which leaves the count and the day as they were.
    let undone = t.ok(&s, "undo", json!({ "batch": second["data"]["batch"] })).await;
    let again = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-11" })).await;
    for batch in [&second, &undone] {
        let r = t.tool(&s, "undo", json!({ "batch": batch["data"]["batch"] })).await;
        assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    }
    let second = again;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["completions"], json!({ "count": 2, "last": "2026-11-11" }));
    assert!(text(&shown).contains("完成次数：2，最近 2026-11-11"), "{}", text(&shown));
    // Undone, the count goes back with the due date; and the time before, one after another.
    t.ok(&s, "undo", json!({ "batch": second["data"]["batch"] })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["completions"], json!({ "count": 1, "last": "2026-11-10" }));
    assert_eq!(shown["data"]["due"]["date"], "2026-11-11");
    let undone = t.ok(&s, "undo", json!({ "batch": first["data"]["batch"] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"]["date"], "2026-11-10");
    t.ok(&s, "undo", json!({ "batch": undone["data"]["batch"] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["completions"], json!({ "count": 1, "last": "2026-11-10" }));

    // Ending the repeat, it is done at once, which isn't one more time, and what waits on it starts:
    // due today, it held that up again.
    let done = t.ok(&s, "done", json!({ "tasks": [1], "end_repeat": true, "today": "2026-11-11" })).await;
    assert!(text(&done).contains("完成 #1 读《三体》") && text(&done).contains("解锁 #2 写读后感"), "{}", text(&done));
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["status"], "done");
    assert_eq!(shown["data"]["due"], json!({ "date": "2026-11-11" }));
    assert_eq!(shown["data"]["completions"], json!({ "count": 1, "last": "2026-11-10" }));
    // Undone, it repeats again.
    t.ok(&s, "undo", json!({ "batch": done["data"]["batch"] })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["status"], "ready");
    assert_eq!(shown["data"]["due"], json!({ "date": "2026-11-11", "repeat": { "every": 1, "unit": "day", "from": "due" } }));
}

#[tokio::test]
async fn each_time_a_repeating_task_is_done_is_recorded_until_undone_or_deleted() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "浇花", "due": "2026-11-10", "repeat": "1d" })).await;
    let records = |snap: Value| {
        snap["completions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["due"].clone(), r["done"].clone(), r["again"].clone()))
            .collect::<Vec<_>>()
    };
    // Late, caught up to today; then early.
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(
        text(&shown).contains(
            "最近的完成记录：\n  2026-11-12 完成，截止 2026-11-13，提前 1 天\n  2026-11-12 完成，截止 2026-11-12，按时\n  2026-11-12 完成，截止 2026-11-10，晚 2 天"
        ),
        "{}",
        text(&shown)
    );
    assert_eq!(shown["data"]["completion_records"][2], json!({ "due": "2026-11-10", "done": "2026-11-12" }));
    // The same time done again is recorded again; undone, that record goes.
    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-10" })).await;
    let again = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-14" })).await;
    let first = (json!("2026-11-10"), json!("2026-11-12"), json!(0));
    let today = (json!("2026-11-12"), json!("2026-11-12"), json!(0));
    let early = (json!("2026-11-13"), json!("2026-11-12"), json!(0));
    let before = [first.clone(), today.clone(), early.clone()];
    assert_eq!(records(t.snapshot(&s).await), [first, (json!("2026-11-10"), json!("2026-11-14"), json!(1)), today, early]);
    t.ok(&s, "undo", json!({ "batch": again["data"]["batch"] })).await;
    assert_eq!(records(t.snapshot(&s).await), before);
    // Ending the repeat is no time done; deleted, the records go with the task, and come back with it.
    t.ok(&s, "done", json!({ "tasks": [1], "end_repeat": true, "today": "2026-11-14" })).await;
    assert_eq!(records(t.snapshot(&s).await), before);
    let deleted = t.ok(&s, "delete", json!({ "tasks": [1] })).await;
    assert!(records(t.snapshot(&s).await).is_empty());
    t.ok(&s, "undo", json!({ "batch": deleted["data"]["batch"] })).await;
    assert_eq!(records(t.snapshot(&s).await), before);
    // Counted from the day it is done, done early twice in a day, it is due the same day each time:
    // a record each, alike but for how often that time was done before.
    t.ok(&s, "add", json!({ "title": "喝水", "due": "2026-11-13", "repeat": "1d", "repeat_from": "done" })).await;
    t.ok(&s, "done", json!({ "tasks": [2], "today": "2026-11-12" })).await;
    let second = t.ok(&s, "done", json!({ "tasks": [2], "today": "2026-11-12" })).await;
    let water = |again: u32| (json!("2026-11-13"), json!("2026-11-12"), json!(again));
    assert_eq!(records(t.snapshot(&s).await)[3..], [water(0), water(1)]);
    t.ok(&s, "undo", json!({ "batch": second["data"]["batch"] })).await;
    assert_eq!(records(t.snapshot(&s).await)[3..], [water(0)]);
}

#[tokio::test]
async fn a_repeat_until_a_day_ends_done_the_last_time() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "练一套卷子", "due": "2026-12-18", "repeat": "1d", "repeat_until": "2026-12-19" })).await;
    t.ok(&s, "add", json!({ "title": "参加考试", "due": "2026-12-20", "after": [1] })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(text(&shown).contains("截止：2026-12-18，每天，到 2026-12-19"), "{}", text(&shown));
    // Never due after it, nor without a repeat.
    let r = t.tool(&s, "edit", json!({ "task": 1, "due": "2026-12-20" })).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("due")), "{}", r.body);
    let r = t.tool(&s, "add", json!({ "title": "x", "due": "2026-12-20", "repeat": "1d", "repeat_until": "2026-12-19" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    let r = t.tool(&s, "edit", json!({ "task": 1, "repeat_until": "2026-12-25" })).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("repeat_until")), "{}", r.body);
    t.ok(&s, "add", json!({ "title": "上晚自习", "due": "2026-12-18 19:00", "repeat": "1d" })).await;
    for remind in [15, 43_201] {
        let r = t.tool(&s, "edit", json!({ "task": 3, "remind": remind, "repeat": "1d", "repeat_until": "2026-12-17" })).await;
        let said = (r.status, r.body["details"]["field"].as_str(), r.body["message"].as_str());
        assert_eq!(said, (StatusCode::BAD_REQUEST, Some("repeat_until"), Some("重复到不能早于截止日期")), "{}", r.body);
    }

    let done = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-12-18" })).await;
    assert!(text(&done).starts_with("顺延 #1 练一套卷子 → 2026-12-19"), "{}", text(&done));
    // The last time it is done for good, counted as one more; what waits on it starts and stays so.
    let last = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-12-19" })).await;
    assert!(text(&last).starts_with("完成 #1 练一套卷子\n"), "{}", text(&last));
    assert!(text(&last).contains("解锁 #2 参加考试") && !text(&last).contains("修改"), "{}", text(&last));
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["status"], "done");
    assert_eq!(shown["data"]["due"], json!({ "date": "2026-12-19" }));
    assert_eq!(shown["data"]["completions"], json!({ "count": 2, "last": "2026-12-19" }));
    assert_eq!(t.ok(&s, "show", json!({ "task": 2, "today": "2026-12-25" })).await["data"]["status"], "ready");
    // Undone, it is due the last time again.
    t.ok(&s, "undo", json!({ "batch": last["data"]["batch"] })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["status"], "ready");
    let repeat = json!({ "every": 1, "unit": "day", "from": "due", "until": "2026-12-19" });
    assert_eq!(shown["data"]["due"], json!({ "date": "2026-12-19", "repeat": repeat }));
    assert_eq!(shown["data"]["completions"], json!({ "count": 1, "last": "2026-12-18" }));
    // A repeat given anew is whole: without the day, it goes on and on.
    t.ok(&s, "edit", json!({ "task": 1, "repeat": "1d" })).await;
    let due = t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"].clone();
    assert_eq!(due, json!({ "date": "2026-12-19", "repeat": { "every": 1, "unit": "day", "from": "due" } }));
}

#[tokio::test]
async fn a_time_skipped_moves_on_without_counting_as_done() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "开周会", "start": "2026-11-15", "due": "2026-11-16", "repeat": "1w", "repeat_until": "2026-11-30" }))
        .await;
    t.ok(&s, "add", json!({ "title": "写周报", "after": [1] })).await;
    t.ok(&s, "add", json!({ "title": "交房租", "due": "2026-11-16" })).await;
    let r = t.tool(&s, "skip", json!({ "tasks": [1] })).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("today")), "{}", r.body);
    assert_eq!(t.tool(&s, "skip", json!({ "tasks": [3], "today": "2026-11-18" })).await.status, StatusCode::CONFLICT);

    // Missed on Monday, it is next week's, the start as far before; nothing waiting on it starts.
    let skipped = t.ok(&s, "skip", json!({ "tasks": [1], "today": "2026-11-18" })).await;
    assert!(text(&skipped).starts_with("顺延 #1 开周会 → 2026-11-23") && !text(&skipped).contains("解锁"), "{}", text(&skipped));
    let shown = t.ok(&s, "show", json!({ "task": 1, "today": "2026-11-18" })).await;
    assert_eq!((shown["data"]["start"].as_str(), shown["data"]["due"]["date"].as_str()), (Some("2026-11-22"), Some("2026-11-23")));
    assert_eq!(shown["data"]["completions"], Value::Null);
    assert!(t.snapshot(&s).await["completions"].as_array().unwrap().is_empty());
    assert_eq!(t.ok(&s, "show", json!({ "task": 2 })).await["data"]["status"], "waiting");

    // Skipped early, one counted from the day done moves on from its due date, not back.
    t.ok(&s, "add", json!({ "title": "换滤芯", "start": "2026-12-20", "due": "2026-12-22", "repeat": "30d", "repeat_from": "done" })).await;
    let filter = t.ok(&s, "skip", json!({ "tasks": [4], "today": "2026-11-22" })).await;
    assert!(text(&filter).starts_with("顺延 #4 换滤芯 → 2027-01-21"), "{}", text(&filter));
    assert_eq!(t.ok(&s, "show", json!({ "task": 4 })).await["data"]["start"], "2027-01-19");

    let again = t.ok(&s, "skip", json!({ "tasks": [1], "today": "2026-11-23" })).await;
    let r = t.tool(&s, "skip", json!({ "tasks": [1], "today": "2026-11-30" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "the last time can't be skipped: {}", r.body);
    t.ok(&s, "undo", json!({ "batch": again["data"]["batch"] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"]["date"], "2026-11-23");
}

#[tokio::test]
async fn a_repeat_by_the_lunar_calendar_moves_on_by_it_and_says_so() {
    let t = TestApp::new();
    let s = t.admin().await;
    // 2026-09-25 is 八月十五 (fixtures/lunar.json has the calendar checked).
    t.ok(&s, "add", json!({ "title": "给妈妈打电话", "due": "2026-09-25", "repeat": "1ly:15" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(text(&shown).contains("截止：2026-09-25（农历八月十五），每年农历八月十五"), "{}", text(&shown));
    let repeat = json!({ "every": 12, "unit": "month", "from": "due", "days": [15], "lunar": true });
    assert_eq!(shown["data"]["due"], json!({ "date": "2026-09-25", "repeat": repeat }));
    let done = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-09-25" })).await;
    assert!(text(&done).starts_with("顺延 #1 给妈妈打电话 → 2027-09-15"), "{}", text(&done));
    t.ok(&s, "edit", json!({ "task": 1, "repeat": "1lm:1,15" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(text(&shown).contains("截止：2027-09-15（农历八月十五），农历每月初一、十五"), "{}", text(&shown));
    // Always on some days, 初一 to 三十; and only monthly.
    for repeat in ["1lm", "1ly", "1lm:31", "1lw:1"] {
        let r = t.tool(&s, "edit", json!({ "task": 1, "repeat": repeat })).await;
        assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("repeat")), "{repeat}: {}", r.body);
    }
    // Within the calendar's table, given so or set back by an undo.
    let r = t.tool(&s, "add", json!({ "title": "x", "due": "2101-02-01", "repeat": "1lm:1" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    t.ok(&s, "add", json!({ "title": "百年后", "due": "2101-02-01" })).await;
    let r = t.tool(&s, "edit", json!({ "task": 2, "repeat": "1lm:1" })).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("repeat")), "{}", r.body);
    let moved = t.ok(&s, "edit", json!({ "task": 2, "due": "2101-01-28" })).await;
    t.ok(&s, "edit", json!({ "task": 2, "repeat": "1ly:29" })).await;
    let r = t.tool(&s, "undo", json!({ "batch": moved["data"]["batch"] })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert!(text(&t.ok(&s, "show", json!({ "task": 2 })).await).contains("截止：2101-01-28（农历腊月廿九）"));
}

#[tokio::test]
async fn a_task_waits_for_its_start_date_and_keeps_it_as_far_before_its_due_date() {
    let t = TestApp::new();
    let s = t.admin().await;
    let today = "2026-11-12";
    t.ok(&s, "add", json!({ "title": "续费域名", "start": "2026-11-20", "due": "2026-12-20", "repeat": "1y" })).await;
    t.ok(&s, "add", json!({ "title": "领券", "start": today, "due": today })).await;
    let late = t.tool(&s, "add", json!({ "title": "晚了", "start": "2026-11-21", "due": "2026-11-20" })).await;
    assert_eq!((late.status, late.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("start")));

    // Not started, it waits; the day it starts, it can be done.
    let next = t.ok(&s, "next", json!({ "today": today })).await;
    assert_eq!(numbers(&next["data"]["tasks"]), [2]);
    let shown = t.ok(&s, "show", json!({ "task": 1, "today": today })).await;
    assert_eq!((shown["data"]["status"].as_str(), shown["data"]["start"].as_str()), (Some("waiting"), Some("2026-11-20")));
    assert!(text(&shown).starts_with("#1 [2026-11-20 开始] 续费域名\n") && text(&shown).contains("开始：2026-11-20"), "{}", text(&shown));
    assert_eq!(numbers(&t.ok(&s, "next", json!({ "today": "2026-11-20" })).await["data"]["tasks"]), [2, 1]);

    // Done, it moves on a year, and the start date with it.
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-25" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1, "today": "2026-11-25" })).await;
    assert_eq!((shown["data"]["start"].as_str(), shown["data"]["due"]["date"].as_str()), (Some("2027-11-20"), Some("2027-12-20")));
    assert_eq!(shown["data"]["status"], "waiting", "it waits for next time");
    t.ok(&s, "edit", json!({ "task": 1, "start": null })).await;
    assert!(t.ok(&s, "show", json!({ "task": 1 })).await["data"].get("start").is_none());
}

#[tokio::test]
async fn a_task_with_a_start_time_waits_for_that_moment_and_keeps_the_time() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "抢票", "start": "2026-11-12 20:00", "due": "2026-11-13", "repeat": "1d" })).await;
    // A due date without a time lasts all day; one with a time ends there.
    t.ok(&s, "add", json!({ "title": "领券", "start": "2026-11-12 23:00", "due": "2026-11-12" })).await;
    let late = t.tool(&s, "add", json!({ "title": "晚了", "start": "2026-11-12 19:00", "due": "2026-11-12 18:00" })).await;
    assert_eq!((late.status, late.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("start_time")));

    // Before the time it waits, from it it can be done; with only the day known, it has started.
    let shown = t.ok(&s, "show", json!({ "task": 1, "today": "2026-11-12", "time": "19:59" })).await;
    assert_eq!((shown["data"]["status"].as_str(), shown["data"]["start_time"].as_str()), (Some("waiting"), Some("20:00")));
    assert!(
        text(&shown).starts_with("#1 [2026-11-12 20:00 开始] 抢票\n") && text(&shown).contains("开始：2026-11-12 20:00"),
        "{}",
        text(&shown)
    );
    // With it, when the next one starts, for the widget to fetch it again then.
    let at_eight = t.ok(&s, "next", json!({ "today": "2026-11-12", "time": "20:00" })).await["data"].clone();
    assert_eq!((numbers(&at_eight["tasks"]), at_eight["next_start"].as_str()), (vec![1], Some("2026-11-12 23:00")));
    let all_day = t.ok(&s, "next", json!({ "today": "2026-11-12" })).await["data"].clone();
    assert_eq!((numbers(&all_day["tasks"]), all_day.get("next_start")), (vec![2, 1], None));
    let alone = t.tool(&s, "next", json!({ "time": "20:00" })).await;
    assert_eq!((alone.status, alone.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("today")));

    // Done, it moves on a day, at the same time.
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-13" })).await;
    let moved = t.ok(&s, "show", json!({ "task": 1 })).await["data"].clone();
    assert_eq!(
        (moved["start"].as_str(), moved["start_time"].as_str(), moved["due"]["date"].as_str()),
        (Some("2026-11-13"), Some("20:00"), Some("2026-11-14"))
    );

    // A client that knows only dates moves the date and keeps the time; the edit tool, given only a
    // date, starts at the start of that day.
    let task = |snap: Value| snap["tasks"].as_array().unwrap().iter().find(|t| t["number"] == 1).unwrap().clone();
    let id = task(t.snapshot(&s).await)["id"].clone();
    assert_eq!(t.mutate(&s, json!([{ "op": "task.update", "id": id, "start": "2026-11-12" }])).await.status, StatusCode::OK);
    assert_eq!(task(t.snapshot(&s).await)["start_time"], "20:00");
    t.ok(&s, "edit", json!({ "task": 1, "start": "2026-11-13" })).await;
    let edited = task(t.snapshot(&s).await);
    assert_eq!((edited["start"].as_str(), edited.get("start_time")), (Some("2026-11-13"), None));

    // Its time can go alone, but can't be without a date.
    t.ok(&s, "edit", json!({ "task": 1, "start": "2026-11-13 08:00" })).await;
    assert_eq!(t.mutate(&s, json!([{ "op": "task.update", "id": id, "start_time": null }])).await.status, StatusCode::OK);
    let cleared = task(t.snapshot(&s).await);
    assert_eq!((cleared["start"].as_str(), cleared.get("start_time")), (Some("2026-11-13"), None));
    let dateless = t.mutate(&s, json!([{ "op": "task.update", "id": id, "start": null, "start_time": "08:00" }])).await;
    assert_eq!((dateless.status, dateless.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("start_time")));

    // Cleared and undone, the date and the time come back.
    t.ok(&s, "edit", json!({ "task": 1, "start": "2026-11-13 08:00" })).await;
    let gone = t.ok(&s, "edit", json!({ "task": 1, "start": null })).await;
    t.ok(&s, "undo", json!({ "batch": gone["data"]["batch"] })).await;
    let back = task(t.snapshot(&s).await);
    assert_eq!((back["start"].as_str(), back["start_time"].as_str()), (Some("2026-11-13"), Some("08:00")));
    // Undoing a new time once the date is gone brings back no time without a date.
    let later = t.ok(&s, "edit", json!({ "task": 1, "start": "2026-11-13 09:00" })).await;
    t.ok(&s, "edit", json!({ "task": 1, "start": null })).await;
    t.ok(&s, "undo", json!({ "batch": later["data"]["batch"] })).await;
    let undone = task(t.snapshot(&s).await);
    assert_eq!((undone.get("start"), undone.get("start_time")), (Some(&Value::Null), None));
}

#[tokio::test]
async fn next_lists_only_the_folders_asked_for() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "写周报", "folder": "/工作" })).await;
    t.ok(&s, "add", json!({ "title": "改首页", "folder": "/工作/网站" })).await;
    t.ok(&s, "add", json!({ "title": "背单词", "folder": "/学习" })).await;
    t.ok(&s, "add", json!({ "title": "买菜" })).await;
    let titles = async |folders: Value| {
        let next = t.ok(&s, "next", json!({ "folders": folders })).await;
        next["data"]["tasks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
    };
    assert_eq!(titles(json!(["/工作"])).await, ["写周报", "改首页"], "with its subfolders");
    assert_eq!(titles(json!(["/学习"])).await, ["背单词"]);
    assert_eq!(titles(json!(["/学习", "/"])).await, ["买菜", "写周报", "改首页", "背单词"], "the root is all of them");
    assert_eq!(titles(json!(["/旧项目", "/"])).await.len(), 4, "also with a folder gone");
    let none = t.ok(&s, "next", json!({ "folders": ["/学习"], "limit": 1 })).await;
    assert_eq!(none["data"]["total"], 1);
    t.ok(&s, "done", json!({ "tasks": [3] })).await;
    assert_eq!(text(&t.ok(&s, "next", json!({ "folders": ["/学习"] })).await), "这些文件夹里暂无可做的事项。");
    assert_eq!(t.tool(&s, "next", json!({ "folders": ["/生活"] })).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn agenda_lists_what_is_due_each_day_and_what_is_overdue() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "交报告", "folder": "/工作", "due": "2026-11-18" })).await;
    t.ok(&s, "add", json!({ "title": "开例会", "folder": "/工作", "due": "2026-11-20 10:00", "repeat": "1w" })).await;
    t.ok(&s, "add", json!({ "title": "买菜", "due": "2026-11-21" })).await;
    t.ok(&s, "add", json!({ "title": "背单词" })).await;
    let agenda = t.ok(&s, "agenda", json!({ "today": "2026-11-20", "days": 8 })).await;
    assert_eq!(
        text(&agenda),
        "已逾期：\n  #1 [可做] 交报告 · /工作 · 截止 2026-11-18\n2026-11-20（今天）：\n  #2 [可做] 开例会 · /工作 · 截止 2026-11-20 10:00，每周\n\
         2026-11-21（明天）：\n  #3 [可做] 买菜 · 截止 2026-11-21\n2026-11-27：\n  #2 开例会 · /工作 · 截止 2026-11-20 10:00，每周 · 以后的一次"
    );
    let data = &agenda["data"];
    assert_eq!(numbers(&data["overdue"]), [1]);
    let dates: Vec<&str> = data["days"].as_array().unwrap().iter().map(|d| d["date"].as_str().unwrap()).collect();
    assert_eq!(dates, ["2026-11-20", "2026-11-21", "2026-11-27"]);
    let later = &data["days"][2]["tasks"][0];
    assert_eq!((later["later"].as_bool(), later["due"]["date"].as_str()), (Some(true), Some("2026-11-20")), "due as it is now");
    assert!(data["days"][0]["tasks"][0].get("later").is_none());

    let work = t.ok(&s, "agenda", json!({ "today": "2026-11-20", "folders": ["/工作"] })).await;
    assert_eq!((numbers(&work["data"]["overdue"]), numbers(&work["data"]["days"][0]["tasks"])), (vec![1], vec![2]), "a week, in /工作");
    assert_eq!(work["data"]["days"].as_array().unwrap().len(), 1);
    let none = t.ok(&s, "agenda", json!({ "today": "2026-11-01", "days": 1 })).await;
    assert_eq!(text(&none), "今天起 1 天没有截止的事项，也没有逾期的。");
    assert_eq!(none["data"], json!({ "overdue": [], "days": [] }));
    for days in [0, 32] {
        assert_eq!(t.tool(&s, "agenda", json!({ "days": days })).await.status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn next_says_which_step_a_task_is() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "开通账号" })).await;
    t.ok(&s, "add", json!({ "title": "配置", "after": [1] })).await;
    t.ok(&s, "add", json!({ "title": "上线", "after": [2] })).await;
    t.ok(&s, "add", json!({ "title": "买菜" })).await;
    t.ok(&s, "done", json!({ "tasks": [1] })).await;
    let next = t.ok(&s, "next", json!({})).await;
    assert_eq!(text(&next), "#2 配置 · 第 2/3 步 · 解锁 1 项\n#4 买菜");
    assert_eq!(next["data"]["tasks"][0]["step"], json!([2, 3]));
    assert!(next["data"]["tasks"][1].get("step").is_none());
}

#[tokio::test]
async fn a_repeating_task_done_moves_on_wherever_its_next_time_falls() {
    let t = TestApp::new();
    let s = t.admin().await;
    // Counted from the day done and done early: to the same day, or to one before its due date.
    for (task, due, next) in [(1, "2026-11-21", "2026-11-21"), (2, "2026-11-22", "2026-11-21")] {
        t.ok(&s, "add", json!({ "title": "吃药", "due": due, "repeat": "1d", "repeat_from": "done" })).await;
        let done = t.ok(&s, "done", json!({ "tasks": [task], "today": "2026-11-20" })).await;
        assert_eq!(done["data"]["changes"], json!([{ "kind": "task_repeated", "task": task, "due": next }]));
        let undone = t.ok(&s, "undo", json!({ "batch": done["data"]["batch"] })).await;
        let fields = if due == next { json!(["completions"]) } else { json!(["due", "completions"]) };
        assert_eq!(undone["data"]["changes"], json!([{ "kind": "task_edited", "task": task, "fields": fields }]));
    }
    let log = t.ok(&s, "log", json!({ "limit": 2 })).await["data"].clone();
    assert_eq!(
        (log[0]["summary"].as_str(), log[1]["summary"].as_str()),
        (Some("撤销批次 5：修改 #2 吃药"), Some("顺延 #2 吃药 → 2026-11-21"))
    );

    // Its start date moves along with it, which is no edit; a reminder changed as it is done is.
    t.ok(
        &s,
        "add",
        json!({ "title": "复诊", "start": "2026-11-19", "due": "2026-11-21", "remind": "08:00", "repeat": "1d", "repeat_from": "done" }),
    )
    .await;
    let id = t.snapshot(&s).await["tasks"].as_array().unwrap().iter().find(|t| t["number"] == 3).unwrap()["id"].clone();
    let due = json!({ "date": "2026-11-21", "reminders": [{ "days": 0, "time": "09:00" }], "repeat": { "every": 1, "unit": "day", "from": "done" } });
    t.mutate(&s, json!([{ "op": "task.update", "id": id, "due": due, "done": true, "today": "2026-11-20" }])).await;
    let log = t.ok(&s, "log", json!({ "limit": 1 })).await["data"].clone();
    assert_eq!(log[0]["summary"], "顺延 #3 复诊 → 2026-11-21；修改 #3 复诊");
    let done = t.ok(&s, "done", json!({ "tasks": [3], "today": "2026-11-21" })).await;
    assert_eq!(done["data"]["changes"], json!([{ "kind": "task_repeated", "task": 3, "due": "2026-11-22" }]));

    // Made to repeat as it is done, it moves on too; its start cleared as it does, at the first of
    // all dates, it reads all the same.
    let id = async |n: i64| t.snapshot(&s).await["tasks"].as_array().unwrap().iter().find(|t| t["number"] == n).unwrap()["id"].clone();
    let daily = |date: &str| json!({ "date": date, "repeat": { "every": 1, "unit": "day", "from": "done" } });
    t.ok(&s, "add", json!({ "title": "换药", "due": "2026-11-21" })).await;
    t.mutate(&s, json!([{ "op": "task.update", "id": id(4).await, "due": daily("2026-11-21"), "done": true, "today": "2026-11-20" }]))
        .await;
    t.ok(&s, "add", json!({ "title": "很早", "start": "0000-01-01", "due": "0000-01-03", "repeat": "1d", "repeat_from": "done" })).await;
    t.mutate(&s, json!([{ "op": "task.update", "id": id(5).await, "start": null, "done": true, "today": "0000-01-01" }])).await;
    let log = t.ok(&s, "log", json!({ "limit": 3 })).await["data"].clone();
    assert_eq!(
        [&log[0]["summary"], &log[2]["summary"]],
        ["顺延 #5 很早 → 0000-01-02；修改 #5 很早", "顺延 #4 换药 → 2026-11-21；修改 #4 换药"]
    );
}

#[tokio::test]
async fn a_repeating_task_moves_on_when_done() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "开例会", "due": "2026-11-09 10:00", "remind": 15, "repeat": "1w" })).await;
    t.ok(&s, "add", json!({ "title": "写周报", "after": [1] })).await;
    let needs_today = t.tool(&s, "done", json!({ "tasks": [1] })).await;
    assert_eq!((needs_today.status, needs_today.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("today")));

    let done = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-11" })).await;
    assert!(text(&done).starts_with("顺延 #1 开例会 → 2026-11-16\n"), "{}", text(&done));
    assert!(text(&done).contains("\n解锁 #2 写周报\n"), "done this time, it no longer holds up what follows: {}", text(&done));
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["status"], "ready", "it stays open");
    let repeat = json!({ "every": 1, "unit": "week", "from": "due" });
    assert_eq!(shown["data"]["due"], json!({ "date": "2026-11-16", "time": "10:00", "reminders": [{ "before": 15 }], "repeat": repeat }));
    assert!(text(&shown).contains("截止：2026-11-16 10:00，每周，提醒：提前 15 分钟"), "{}", text(&shown));
    let status = async |today: &str| t.ok(&s, "show", json!({ "task": 2, "today": today })).await["data"]["status"].clone();
    assert_eq!(status("2026-11-15").await, "ready");
    assert_eq!(status("2026-11-16").await, "waiting", "the next time come, what follows waits for it again");
    assert!(text(&t.ok(&s, "log", json!({ "limit": 1 })).await).contains("顺延 #1 开例会 → 2026-11-16"));
    // Undone after the reminder changed, it moves back and the reminder stays as changed.
    t.ok(&s, "edit", json!({ "task": 1, "remind": 30 })).await;
    t.ok(&s, "undo", json!({ "batch": done["data"]["batch"] })).await;
    let due = t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"].clone();
    assert_eq!(due, json!({ "date": "2026-11-09", "time": "10:00", "reminders": [{ "before": 30 }], "repeat": repeat }));
    // Undone after the due date was cleared, only the date comes back: the rest stays cleared.
    let done = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-11" })).await;
    t.ok(&s, "edit", json!({ "task": 1, "due": null })).await;
    t.ok(&s, "undo", json!({ "batch": done["data"]["batch"] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"], json!({ "date": "2026-11-09" }));
    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-09 10:00", "repeat": "1w" })).await;

    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-10" })).await;
    assert_eq!(
        t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"],
        json!({ "date": "2026-11-10", "repeat": repeat }),
        "moving it keeps the repeat"
    );
    t.ok(&s, "edit", json!({ "task": 1, "repeat": "30d", "repeat_from": "done" })).await;
    let done = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    assert!(text(&done).starts_with("顺延 #1 开例会 → 2026-12-12"), "{}", text(&done));
    assert!(text(&t.ok(&s, "show", json!({ "task": 1 })).await).contains("截止：2026-12-12，每 30 天，从完成那天算"));

    // Repeating no more, it can be done, and what waits on it can start.
    t.ok(&s, "edit", json!({ "task": 1, "repeat": null })).await;
    let done = t.ok(&s, "done", json!({ "tasks": [1] })).await;
    assert!(text(&done).contains("完成 #1 开例会") && text(&done).contains("解锁 #2 写周报"), "{}", text(&done));
    // Done, it repeats no more: what waits on it has started.
    let r = t.tool(&s, "edit", json!({ "task": 1, "repeat": "1w" })).await;
    assert_eq!((r.status, r.body["message"].as_str()), (StatusCode::BAD_REQUEST, Some("“开例会”已完成，不能设置重复，请先重新打开")));

    for (args, field) in [
        (json!({ "title": "x", "repeat": "1d" }), "repeat"),
        (json!({ "title": "x", "due": "2026-11-20", "repeat": "2周" }), "repeat"),
        (json!({ "title": "x", "due": "2026-11-20", "repeat": "0d" }), "repeat"),
        (json!({ "title": "x", "due": "2026-11-20", "repeat": "1w:8" }), "repeat"),
        (json!({ "title": "x", "due": "2026-11-20", "repeat": "1d:1" }), "repeat"),
        (json!({ "title": "x", "due": "2026-11-20", "repeat": "1m:32" }), "repeat"),
        (json!({ "title": "x", "due": "2026-11-20", "repeat": "1y:1" }), "repeat"),
        (json!({ "title": "x", "due": "2026-11-20", "repeat_from": "done" }), "repeat_from"),
    ] {
        let r = t.tool(&s, "add", args.clone()).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{args}");
        assert!(r.body["details"]["field"].as_str().is_some_and(|f| f.starts_with(field)), "{args}: {}", r.body);
    }
    t.ok(&s, "add", json!({ "title": "交房租", "due": "2026-11-30", "repeat": "1m" })).await;
    assert_eq!(t.tool(&s, "edit", json!({ "task": 3, "due": null, "repeat": "1w" })).await.status, StatusCode::BAD_REQUEST);
    t.ok(&s, "edit", json!({ "task": 3, "due": null })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 3 })).await["data"]["due"], Value::Null, "the repeat goes with the due date");
    assert_eq!(t.tool(&s, "edit", json!({ "task": 3, "repeat": "1m" })).await.status, StatusCode::BAD_REQUEST, "no due date to repeat");

    // Weekly on some days, it moves on to the next of them.
    t.ok(&s, "add", json!({ "title": "写日报", "due": "2026-11-13 18:00", "repeat": "1w:1,2,3,4,5" })).await;
    let done = t.ok(&s, "done", json!({ "tasks": [4], "today": "2026-11-13" })).await;
    assert!(text(&done).starts_with("顺延 #4 写日报 → 2026-11-16"), "{}", text(&done));
    let shown = t.ok(&s, "show", json!({ "task": 4 })).await;
    assert_eq!(shown["data"]["due"]["repeat"]["days"], json!([1, 2, 3, 4, 5]));
    assert!(text(&shown).contains("截止：2026-11-16 18:00，每个工作日"), "{}", text(&shown));
    t.ok(&s, "edit", json!({ "task": 4, "repeat": "2w:4,2" })).await;
    assert!(text(&t.ok(&s, "show", json!({ "task": 4 })).await).contains("截止：2026-11-16 18:00，每 2 周的周二、四"));

    // Monthly on some days, it moves on to the next of them; yearly is every 12 months.
    t.ok(&s, "add", json!({ "title": "领券", "due": "2026-11-27", "repeat": "1m:28,27" })).await;
    let done = t.ok(&s, "done", json!({ "tasks": [5], "today": "2026-11-27" })).await;
    assert!(text(&done).starts_with("顺延 #5 领券 → 2026-11-28"), "{}", text(&done));
    let shown = t.ok(&s, "show", json!({ "task": 5 })).await;
    assert_eq!(shown["data"]["due"]["repeat"]["days"], json!([27, 28]));
    assert!(text(&shown).contains("截止：2026-11-28，每月 27、28 号"), "{}", text(&shown));
    t.ok(&s, "edit", json!({ "task": 5, "repeat": "1y" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 5 })).await;
    assert_eq!(shown["data"]["due"]["repeat"], json!({ "every": 12, "unit": "month", "from": "due" }));
    assert!(text(&shown).contains("截止：2026-11-28，每年"), "{}", text(&shown));
}

#[tokio::test]
async fn lists_search_and_the_log() {
    let t = TestApp::new();
    let s = t.admin().await;
    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "Claude Code", "scope": "write" }))).await.body;
    let agent = token["secret"].as_str().unwrap();
    let plan = json!({ "folder": "/论文", "tasks": [
        { "ref": "draft", "title": "写初稿", "description": "用 Rust 画图" },
        { "title": "改稿", "after": ["draft"] },
    ] });
    t.ok(agent, "apply", json!({ "plan": plan })).await;
    let list = t.ok(&s, "list", json!({ "folder": "论文" })).await;
    assert_eq!(numbers(&list["data"]["tasks"]), vec![1, 2]);
    assert!(text(&list).contains("#2 [等 1 项] 改稿"), "{}", text(&list));
    assert_eq!(t.ok(&s, "search", json!({ "query": "rust" })).await["data"]["total"], 1);

    t.ok(&s, "done", json!({ "tasks": [1] })).await;
    assert_eq!(numbers(&t.ok(&s, "list", json!({ "folder": "/论文" })).await["data"]["tasks"]), vec![2]);
    assert_eq!(numbers(&t.ok(&s, "list", json!({ "folder": "/论文", "all": true })).await["data"]["tasks"]), vec![2, 1]);

    let log = t.ok(&s, "log", json!({})).await["data"].clone();
    assert_eq!((log[0]["actor"].clone(), log[1]["actor"].as_str()), (Value::Null, Some("Claude Code")));
    assert!(log[1]["summary"].as_str().unwrap().contains("添加 2 项"), "{}", log[1]);
    let undo = t.ok(&s, "undo", json!({ "batch": log[0]["batch"] })).await;
    assert!(text(&undo).contains("重新打开 #1 写初稿"), "{}", text(&undo));
    assert!(t.ok(&s, "log", json!({ "limit": 1 })).await["data"][0]["summary"].as_str().unwrap().starts_with("撤销批次"));
}

#[tokio::test]
async fn records_come_back_with_their_task_not_onto_one_back_from_elsewhere() {
    let t = TestApp::new();
    let s = t.admin().await;
    let batch = |r: Value| r["data"]["batch"].clone();
    let records =
        |snap: Value| snap["completions"].as_array().unwrap().iter().map(|r| (r["due"].clone(), r["again"].clone())).collect::<Vec<_>>();
    t.ok(&s, "add", json!({ "title": "喝水", "due": "2026-11-13", "repeat": "1d", "repeat_from": "done" })).await;
    let deleted = batch(t.ok(&s, "delete", json!({ "tasks": [1] })).await);
    t.ok(&s, "undo", json!({ "batch": deleted })).await;
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    let again = batch(t.ok(&s, "delete", json!({ "tasks": [1] })).await);
    // Back as it was before it was done, from the first deletion: undoing the second leaves it so.
    t.ok(&s, "undo", json!({ "batch": deleted })).await;
    t.ok(&s, "undo", json!({ "batch": again })).await;
    let snap = t.snapshot(&s).await;
    assert_eq!((records(snap.clone()), &snap["tasks"][0]["completions"]), (vec![], &Value::Null));
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    assert_eq!(records(t.snapshot(&s).await), [(json!("2026-11-13"), json!(0)), (json!("2026-11-13"), json!(1))]);
}

#[tokio::test]
async fn undoing_a_batch_that_deleted_a_task_and_brought_it_back_as_it_was_earlier_brings_its_records_back() {
    let t = TestApp::new();
    let s = t.admin().await;
    let send = |ops: Value| {
        let (t, s) = (&t, s.clone());
        async move {
            let id = common::id();
            let r = t.call("POST", "/mutations", Some(&s), Some(json!({ "id": id, "ops": ops }))).await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.body);
            id
        }
    };
    let done = |snap: Value| {
        let records: Vec<_> = snap["completions"].as_array().unwrap().iter().map(|r| (r["due"].clone(), r["again"].clone())).collect();
        (records, snap["tasks"][0]["completions"]["count"].clone())
    };
    t.ok(&s, "add", json!({ "title": "喝水", "due": "2026-11-13", "repeat": "1d", "repeat_from": "done" })).await;
    let task = t.snapshot(&s).await["tasks"][0]["id"].clone();
    let deleted = send(json!([{ "op": "task.delete", "id": task }])).await;
    send(json!([{ "op": "undo", "mutation": deleted }])).await;
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    let both = send(json!([{ "op": "task.delete", "id": task }, { "op": "undo", "mutation": deleted }])).await;
    assert_eq!(done(t.snapshot(&s).await), (vec![], Value::Null));
    send(json!([{ "op": "undo", "mutation": both }])).await;
    assert_eq!(done(t.snapshot(&s).await), (vec![(json!("2026-11-13"), json!(0))], json!(1)));
}

#[tokio::test]
async fn undoing_a_time_done_again_as_an_earlier_version_kept_it_brings_the_record_before_back() {
    let t = TestApp::new();
    let s = t.admin().await;
    let done = |snap: Value| {
        let records: Vec<_> = snap["completions"].as_array().unwrap().iter().map(|r| (r["done"].clone(), r["again"].clone())).collect();
        (records, snap["tasks"][0]["completions"]["count"].clone())
    };
    t.ok(&s, "add", json!({ "title": "喝水", "due": "2026-11-13", "repeat": "1d", "repeat_from": "done" })).await;
    t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-12" })).await;
    let again = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-13" })).await["data"]["batch"].as_i64().unwrap();
    // As an earlier version kept it: the count up, then the record before taken out for its own, both
    // without `again`.
    t.state
        .db
        .call(move |c| {
            let effects: String = c.query_row("SELECT effects FROM mutations WHERE seq = ?1", [again], |r| r.get(0))?;
            let mut effects: Vec<Value> = serde_json::from_str(&effects).unwrap();
            let task = effects[0]["after"]["id"].clone();
            let record = |done: &str| json!({ "task_id": task, "due": "2026-11-13", "done": done });
            effects.truncate(1);
            effects.push(json!({ "kind": "completion", "before": record("2026-11-12"), "after": null }));
            effects.push(json!({ "kind": "completion", "before": null, "after": record("2026-11-13") }));
            c.execute("UPDATE mutations SET effects = ?1 WHERE seq = ?2", rusqlite::params![Value::from(effects).to_string(), again])?;
            c.execute("DELETE FROM completions WHERE again = 0", [])?;
            c.execute("UPDATE completions SET again = 0", [])?;
            Ok(())
        })
        .await
        .unwrap();
    let undo = t.ok(&s, "undo", json!({ "batch": again })).await["data"]["batch"].clone();
    assert_eq!(done(t.snapshot(&s).await), (vec![(json!("2026-11-12"), json!(0))], json!(1)));
    t.ok(&s, "undo", json!({ "batch": undo })).await;
    assert_eq!(done(t.snapshot(&s).await), (vec![(json!("2026-11-13"), json!(0))], json!(2)));
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

#[tokio::test]
async fn permissions_idempotency_and_bad_arguments() {
    let t = TestApp::new();
    let s = t.admin().await;
    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "看板", "scope": "read" }))).await.body;
    let reader = token["secret"].as_str().unwrap();
    assert_eq!(t.tool(reader, "next", json!({})).await.status, StatusCode::OK);
    assert_eq!(t.tool(reader, "add", json!({ "title": "x" })).await.status, StatusCode::FORBIDDEN);

    let add = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/tools/add")
            .header(header::AUTHORIZATION, format!("Bearer {s}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", "019a4b2c-0000-7000-8000-000000000042")
            .body(Body::from(json!({ "title": "只加一次" }).to_string()))
            .unwrap()
    };
    let first = t.app.clone().oneshot(add()).await.unwrap().into_body().collect().await.unwrap().to_bytes();
    let again = t.app.clone().oneshot(add()).await.unwrap().into_body().collect().await.unwrap().to_bytes();
    assert_eq!(first, again);
    // The same change, named in the arguments too; and another, which the key and the arguments can't both name.
    let r = t.tool(&s, "add", json!({ "title": "只加一次", "request_id": "019a4b2c-0000-7000-8000-000000000042" })).await;
    assert_eq!(r.body, serde_json::from_slice::<Value>(&first).unwrap());
    let mut mixed = add();
    *mixed.body_mut() = Body::from(json!({ "title": "x", "request_id": "019a4b2c-0000-7000-8000-000000000045" }).to_string());
    let res = t.app.clone().oneshot(mixed).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(t.snapshot(&s).await["tasks"].as_array().unwrap().len(), 1);

    t.ok(&s, "add", json!({ "title": "前置" })).await;
    t.ok(&s, "dep_add", json!({ "before": 2, "after": 1 })).await;
    let done = |key: &'static str, task: i64| {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tools/done")
            .header(header::AUTHORIZATION, format!("Bearer {s}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", key)
            .body(Body::from(json!({ "tasks": [task] }).to_string()))
            .unwrap();
        let app = t.app.clone();
        async move { app.oneshot(req).await.unwrap().into_body().collect().await.unwrap().to_bytes() }
    };
    let first = done("019a4b2c-0000-7000-8000-000000000043", 2).await;
    assert!(String::from_utf8_lossy(&first).contains("解锁 #1"));
    done("019a4b2c-0000-7000-8000-000000000044", 1).await;
    assert_eq!(done("019a4b2c-0000-7000-8000-000000000043", 2).await, first, "a retry gets the first answer, not one made now");

    // A read makes no change: no id to check.
    assert_eq!(t.tool(&s, "next", json!({ "request_id": "这不是 ID" })).await.status, StatusCode::OK);
    let r = t.tool(&s, "show", json!({ "task": "abc" })).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("task")));
    assert_eq!(t.tool(&s, "show", json!({ "task": 1, "extra": true })).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(t.tool(&s, "nope", json!({})).await.status, StatusCode::NOT_FOUND);
    let tools = t.call("GET", "/tools", Some(&s), None).await.body;
    assert!(tools.as_array().unwrap().iter().any(|t| t["name"] == "apply" && t["input_schema"]["properties"]["plan"].is_object()));
}

#[tokio::test]
async fn mcp_serves_the_same_tools() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "交电费" })).await;
    let rpc = |auth: Option<&str>, method: &str, params: Value| {
        let mut req = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::HOST, "todo.example.com")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream");
        if let Some(secret) = auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {secret}"));
        }
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
        let app = t.app.clone();
        async move {
            let res = app.oneshot(req.body(Body::from(body)).unwrap()).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (status, serde_json::from_slice::<Value>(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into())))
        }
    };
    let init = json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } });
    assert_eq!(rpc(None, "initialize", init.clone()).await.0, StatusCode::UNAUTHORIZED);
    let (status, reply) = rpc(Some(&s), "initialize", init).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["result"]["serverInfo"]["name"], "nexttodo");
    assert!(reply["result"]["instructions"].as_str().unwrap().contains("replace"), "{reply}");

    let (_, list) = rpc(Some(&s), "tools/list", json!({})).await;
    let tool = |name: &str| list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap().clone();
    assert_eq!(tool("next")["annotations"]["readOnlyHint"], true);
    assert!(tool("next")["inputSchema"]["properties"].get("request_id").is_none());
    assert!(tool("add")["inputSchema"]["properties"]["request_id"].is_object());

    let (_, called) = rpc(Some(&s), "tools/call", json!({ "name": "next", "arguments": {} })).await;
    assert_eq!(called["result"]["content"][0]["text"], "#1 交电费");
    let (_, failed) = rpc(Some(&s), "tools/call", json!({ "name": "show", "arguments": { "task": 9 } })).await;
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(failed["result"]["content"][0]["text"], "不存在事项 #9");

    // Sent again with its request_id, as when the first answer did not come back: made once.
    let add = json!({ "name": "add", "arguments": { "title": "取快递", "request_id": "019a4b2c-0000-7000-8000-000000000046" } });
    let (_, first) = rpc(Some(&s), "tools/call", add.clone()).await;
    let (_, again) = rpc(Some(&s), "tools/call", add).await;
    assert_eq!(first["result"], again["result"]);
    assert_eq!(t.snapshot(&s).await["tasks"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn editing_part_of_a_description_keeps_what_others_changed_meanwhile() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "写接口", "description": "- [ ] 登录\n- [ ] 列表" })).await;
    let id = t.snapshot(&s).await["tasks"][0]["id"].as_str().unwrap().to_owned();
    // The user adds a line in the app after the agent has read the description.
    t.mutate(&s, json!([{ "op": "task.update", "id": id, "description": "- [ ] 登录\n- [ ] 列表\n- [ ] 退出" }])).await;
    let edited = t
        .ok(&s, "edit", json!({ "task": 1, "replace": [{ "old": "- [ ] 登录", "new": "- [x] 登录" }], "append": "进度：登录已完成" }))
        .await;
    assert_eq!(edited["data"]["changes"], json!([{ "kind": "task_edited", "task": 1, "fields": ["description"] }]));
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["description"], "- [x] 登录\n- [ ] 列表\n- [ ] 退出\n进度：登录已完成");

    let edit = |args: Value| t.tool(&s, "edit", args);
    for (args, status, says) in [
        (json!({ "task": 1, "replace": [{ "old": "- [ ] 注册", "new": "x" }] }), StatusCode::CONFLICT, "先 show"),
        (json!({ "task": 1, "replace": [{ "old": "- [ ]", "new": "x" }] }), StatusCode::BAD_REQUEST, "不止一处"),
        (json!({ "task": 1, "replace": [{ "old": "", "new": "x" }] }), StatusCode::BAD_REQUEST, "不能为空"),
        (json!({ "task": 1, "description": "新的", "append": "x" }), StatusCode::BAD_REQUEST, "整段替换"),
        (json!({ "task": 1, "append": " " }), StatusCode::BAD_REQUEST, "不能为空"),
    ] {
        let r = edit(args).await;
        assert_eq!(r.status, status, "{}", r.body);
        assert!(r.body["message"].as_str().unwrap().contains(says), "{}", r.body);
    }
    t.ok(&s, "edit", json!({ "task": 1, "description": "aaa" })).await;
    let r = edit(json!({ "task": 1, "replace": [{ "old": "aa", "new": "b" }] })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "it overlaps itself: {}", r.body);
    let r = edit(json!({ "task": 1, "replace": [{ "old": "aaa", "new": "b" }, { "old": "aaa", "new": "c" }] })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "one after the other, and all or nothing: {}", r.body);
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["description"], "aaa");
}

#[tokio::test]
async fn lists_stop_at_the_limit_and_say_how_many_more() {
    let t = TestApp::new();
    let s = t.admin().await;
    let plan = json!({ "folder": "/论文", "tasks": [{ "title": "查资料" }, { "title": "写初稿" }, { "title": "改稿" }] });
    t.ok(&s, "apply", json!({ "plan": plan })).await;
    let next = t.ok(&s, "next", json!({ "limit": 2 })).await;
    assert_eq!((numbers(&next["data"]["tasks"]), next["data"]["total"].as_i64()), (vec![1, 2], Some(3)));
    assert!(text(&next).ends_with("\n另有 1 项未列出，可调大 limit"), "{}", text(&next));
    let list = t.ok(&s, "list", json!({ "folder": "/论文", "limit": 1 })).await;
    assert_eq!(text(&list), "/论文\n  #1 [可做] 查资料\n另有 2 项未列出，可调大 limit");
    assert_eq!(list["data"]["total"], 3);
    let found = t.ok(&s, "search", json!({ "query": "稿", "limit": 1 })).await;
    assert_eq!(text(&found), "找到 2 项：\n#2 [可做] 写初稿 · /论文\n另有 1 项未列出，可调大 limit");
    assert!(!text(&t.ok(&s, "next", json!({})).await).contains("另有"));
    for limit in [0, 501] {
        assert_eq!(t.tool(&s, "next", json!({ "limit": limit })).await.status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn outputs_leave_out_what_says_nothing() {
    let t = TestApp::new();
    let s = t.admin().await;
    let long = "把这一大段需求整理成待办事项，再按先后顺序排好，最后把每一项的完成标准都写清楚";
    let added = t.ok(&s, "add", json!({ "title": long })).await;
    assert!(text(&added).starts_with("添加 #1 把这一大段需求整理成待办事项，再按先后顺序排好，最后把每一项…\n"), "{}", text(&added));
    assert_eq!(added["data"]["changes"], json!([{ "kind": "task_created", "task": 1 }]));
    assert!(text(&t.ok(&s, "next", json!({})).await).contains(long), "a list shows the whole title");

    t.ok(&s, "add", json!({ "title": "交初稿", "after": [1] })).await;
    let shown = t.ok(&s, "show", json!({ "task": 2 })).await;
    assert!(
        text(&shown).contains("前置：\n  #1 [可做] 把这一大段需求整理成待办事项，再按先后顺序排好，最后把每一项…\n"),
        "{}",
        text(&shown)
    );
    let data = &shown["data"];
    let keys: Vec<&str> = data.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["before", "created_at", "folder", "id", "number", "status", "title", "updated_at", "waiting_on"]);
    assert_eq!(
        data["before"][0].as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["folder", "id", "number", "status", "title", "unlocks"]
    );

    let done = t.ok(&s, "done", json!({ "tasks": [1] })).await;
    assert!(text(&done).contains("\n解锁 #2 交初稿\n"), "{}", text(&done));
    assert_eq!(done["data"]["changes"], json!([{ "kind": "task_done", "task": 1 }]));
    assert_eq!(numbers(&done["data"]["unlocked"]), vec![2]);
    assert_eq!(t.ok(&s, "reopen", json!({ "tasks": [1] })).await["data"].get("unlocked"), None);
}

#[tokio::test]
async fn a_change_that_does_several_things_reports_each() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "交初稿" })).await;
    let task = t.snapshot(&s).await["tasks"][0]["id"].as_str().unwrap().to_owned();
    let (a, b) = (common::id(), common::id());
    t.mutate(&s, json!([{ "op": "folder.create", "id": a, "name": "A" }, { "op": "folder.create", "id": b, "name": "B" }])).await;
    // The app can change several things in one operation; undoing it changes them all back.
    t.mutate(&s, json!([{ "op": "task.update", "id": task, "title": "交终稿", "done": true }])).await;
    let undone = t.ok(&s, "undo", json!({ "batch": 3 })).await;
    assert_eq!(
        undone["data"]["changes"],
        json!([{ "kind": "task_reopened", "task": 1 }, { "kind": "task_edited", "task": 1, "fields": ["title"] }])
    );
    assert!(text(&undone).starts_with("重新打开 #1 交初稿\n修改 #1 交初稿\n"), "{}", text(&undone));
    t.mutate(&s, json!([{ "op": "folder.update", "id": a, "name": "Z", "parent_id": b }])).await;
    let undone = t.ok(&s, "undo", json!({ "batch": 5 })).await;
    assert_eq!(
        undone["data"]["changes"],
        json!([{ "kind": "folder_renamed", "folder": "/B/Z", "name": "A" }, { "kind": "folder_moved", "folder": "/B/Z", "to": "/A" }])
    );
    t.mutate(&s, json!([{ "op": "folder.update", "id": a, "name": "X", "sort_key": "a5" }])).await;
    let undone = t.ok(&s, "undo", json!({ "batch": 7 })).await;
    assert_eq!(
        undone["data"]["changes"],
        json!([{ "kind": "folder_renamed", "folder": "/X", "name": "A" }, { "kind": "folder_sorted", "folder": "/X" }])
    );
}

#[tokio::test]
async fn a_folder_copies_with_all_in_it_open_again() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "外部审批", "folder": "/其他" })).await;
    t.ok(&s, "add", json!({ "title": "审价", "folder": "/合同", "after": [1] })).await;
    t.ok(
        &s,
        "add",
        json!({ "title": "盖章", "folder": "/合同/盖章", "due": "2026-11-20 18:00", "remind": 60, "repeat": "1w", "after": [2] }),
    )
    .await;
    t.ok(&s, "attach", json!({ "task": 2, "name": "报价.txt", "data": "5oql5Lu3" })).await;
    let old = t.ok(&s, "show", json!({ "task": 2 })).await["data"]["attachments"][0]["id"].as_str().unwrap().to_owned();
    t.ok(&s, "edit", json!({ "task": 2, "description": format!("见[报价](attachment:{old})") })).await;
    t.ok(&s, "done", json!({ "tasks": [1, 2] })).await;
    t.ok(&s, "done", json!({ "tasks": [3], "today": "2026-11-20" })).await;

    let copied = t.ok(&s, "copy", json!({ "folder": "/合同" })).await;
    assert!(text(&copied).contains("合同 副本"), "{}", text(&copied));
    let list = t.ok(&s, "list", json!({ "folder": "/合同 副本", "all": true })).await["data"].clone();
    assert_eq!(list["folders"].as_array().unwrap().len(), 1, "{list}");
    let review = list["tasks"][0].clone();
    assert_eq!(
        (review["title"].as_str(), review["status"].as_str()),
        (Some("审价"), Some("ready")),
        "open again, the outside link left behind"
    );
    let review = t.ok(&s, "show", json!({ "task": review["number"] })).await["data"].clone();
    let new = review["attachments"][0]["id"].as_str().unwrap();
    assert_ne!(new, old);
    assert_eq!(review["description"], format!("见[报价](attachment:{new})"));
    assert_eq!(t.download(&s, new).await.2, "报价".as_bytes());
    let stamp = t.ok(&s, "list", json!({ "folder": "/合同 副本/盖章" })).await["data"]["tasks"][0].clone();
    let stamp = t.ok(&s, "show", json!({ "task": stamp["number"] })).await["data"].clone();
    assert_eq!(
        stamp["due"],
        json!({ "date": "2026-11-27", "time": "18:00", "reminders": [{ "before": 60 }], "repeat": { "every": 1, "unit": "week", "from": "due" } })
    );
    assert_eq!(stamp["completions"], Value::Null, "counted from nothing again");
    assert_eq!(stamp["waiting_on"], 1, "it waits for the copy of 审价");

    let again = t.ok(&s, "copy", json!({ "folder": "/合同", "to": "/其他" })).await;
    t.ok(&s, "copy", json!({ "folder": "/合同" })).await;
    let tree = text(&t.ok(&s, "tree", json!({})).await).to_owned();
    for path in ["其他", "合同", "合同 副本", "合同 副本 2"] {
        assert!(tree.lines().any(|l| l.trim_start().starts_with(path)), "{path} in {tree}");
    }
    assert_eq!(t.tool(&s, "copy", json!({ "folder": "/" })).await.status, StatusCode::BAD_REQUEST);
    t.ok(&s, "undo", json!({ "batch": again["data"]["batch"] })).await;
    assert!(t.ok(&s, "list", json!({ "folder": "/其他" })).await["data"]["folders"].as_array().unwrap().is_empty());

    // Undone after something was added to it, the copy goes but what was added stays, with the
    // folders it is in.
    let copied = t.ok(&s, "copy", json!({ "folder": "/合同", "to": "/其他" })).await;
    t.ok(&s, "add", json!({ "title": "复制后加的", "folder": "/其他/合同/盖章" })).await;
    t.ok(&s, "undo", json!({ "batch": copied["data"]["batch"] })).await;
    let tree = text(&t.ok(&s, "tree", json!({})).await).to_owned();
    let kept = t.ok(&s, "list", json!({ "folder": "/其他/合同", "all": true })).await["data"].clone();
    assert_eq!((kept["tasks"].as_array().unwrap().len(), kept["folders"][0]["name"].as_str()), (0, Some("盖章")), "{tree}");
    let added = t.ok(&s, "list", json!({ "folder": "/其他/合同/盖章", "all": true })).await["data"]["tasks"].clone();
    assert_eq!(added.as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect::<Vec<_>>(), ["复制后加的"]);
    // One moved into another it made meanwhile, empty, they all go.
    for path in ["/流程/甲", "/流程/乙"] {
        t.ok(&s, "mkdir", json!({ "path": path })).await;
    }
    let copied = t.ok(&s, "copy", json!({ "folder": "/流程" })).await;
    t.ok(&s, "move", json!({ "folders": ["/流程 副本/甲"], "to": "/流程 副本/乙" })).await;
    t.ok(&s, "undo", json!({ "batch": copied["data"]["batch"] })).await;
    assert!(!text(&t.ok(&s, "tree", json!({})).await).contains("流程 副本"));
    // Made in place of one deleted, it goes before that one is back, freeing the name.
    let (old, new) = (common::id(), common::id());
    t.mutate(&s, json!([{ "op": "folder.create", "id": old, "name": "工作" }])).await;
    let swap = t.mutate(&s, json!([{ "op": "folder.delete", "id": old }, { "op": "folder.create", "id": new, "name": "工作" }])).await;
    t.ok(&s, "undo", json!({ "batch": swap.body["seq"] })).await;
    let ids = async || t.snapshot(&s).await["folders"].as_array().unwrap().iter().map(|f| f["id"].clone()).collect::<Vec<_>>();
    let found = ids().await;
    assert!(found.contains(&json!(old)) && !found.contains(&json!(new)));
    // Holding a task it made, moved in since, it waits for the task to go in turn, after what was done
    // to the task is undone.
    let (old, new, folder) = (common::id(), common::id(), common::id());
    t.mutate(&s, json!([{ "op": "task.create", "id": old, "title": "旧的" }])).await;
    let file = t.attach(&s, &old, "发票.txt", b"abc").await;
    let made = t
        .mutate(
            &s,
            json!([
                { "op": "task.create", "id": new, "title": "新的" },
                { "op": "attachment.update", "id": file, "task_id": new },
                { "op": "dep.add", "before": old, "after": new },
                { "op": "dep.remove", "before": old, "after": new },
                { "op": "folder.create", "id": folder, "name": "归档" },
            ]),
        )
        .await;
    t.mutate(&s, json!([{ "op": "task.update", "id": new, "folder_id": folder }])).await;
    t.ok(&s, "undo", json!({ "batch": made.body["seq"] })).await;
    let after = t.snapshot(&s).await;
    let has = |kind: &str, id: &str| after[kind].as_array().unwrap().iter().any(|x| x["id"] == json!(id));
    let on = after["attachments"].as_array().unwrap().iter().find(|a| a["id"] == json!(file)).map(|a| a["task_id"].clone());
    let linked = after["dependencies"].as_array().unwrap().iter().any(|d| d["after"] == json!(new));
    assert_eq!((on, has("tasks", &new), has("folders", &folder), linked), (Some(json!(old)), false, false, false));
    // One it deleted, its ID taken again by one it made that holds what was put in it since, can't
    // come back: nothing is undone.
    let (folder, task) = (common::id(), common::id());
    t.mutate(&s, json!([{ "op": "folder.create", "id": folder, "name": "原文件夹" }])).await;
    let swap = t
        .mutate(
            &s,
            json!([
                { "op": "task.create", "id": task, "title": "临时事项" },
                { "op": "folder.delete", "id": folder },
                { "op": "folder.create", "id": folder, "name": "替换文件夹" },
            ]),
        )
        .await;
    t.mutate(&s, json!([{ "op": "task.update", "id": task, "folder_id": folder }])).await;
    let r = t.tool(&s, "undo", json!({ "batch": swap.body["seq"] })).await;
    assert_eq!(
        (r.status, r.body["message"].as_str()),
        (StatusCode::CONFLICT, Some("文件夹“原文件夹”无法恢复：之后又往取代它的文件夹里放了东西，请先移走"))
    );
    assert!(text(&t.ok(&s, "tree", json!({})).await).contains("替换文件夹"));

    // As long as a name may be, it is cut short to leave room for the rest.
    let long = "長".repeat(100);
    t.ok(&s, "mkdir", json!({ "path": format!("/{long}") })).await;
    t.ok(&s, "copy", json!({ "folder": format!("/{long}") })).await;
    let copy = format!("{} 副本", "長".repeat(97));
    assert!(text(&t.ok(&s, "tree", json!({})).await).lines().any(|l| l.trim_start().starts_with(&copy)));
}

#[tokio::test]
async fn a_folder_takes_a_colour_and_an_icon_that_copies_keep() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "复习", "folder": "/学习" })).await;
    let styled = t.ok(&s, "style", json!({ "folder": "/学习", "color": "blue", "icon": "book" })).await;
    t.ok(&s, "copy", json!({ "folder": "/学习" })).await;
    let looks = |tree: Value| {
        tree.as_array().unwrap().iter().map(|f| (f["name"].clone(), f["color"].clone(), f["icon"].clone())).collect::<Vec<_>>()
    };
    assert_eq!(
        looks(t.ok(&s, "tree", json!({})).await["data"].clone()),
        [
            (json!("/"), Value::Null, Value::Null),
            (json!("学习"), json!("blue"), json!("book")),
            (json!("学习 副本"), json!("blue"), json!("book"))
        ]
    );
    // Back to the default, one at a time.
    t.ok(&s, "style", json!({ "folder": "/学习", "color": null })).await;
    assert_eq!(looks(t.ok(&s, "tree", json!({})).await["data"].clone())[1], (json!("学习"), Value::Null, json!("book")));
    t.ok(&s, "undo", json!({ "batch": styled["data"]["batch"] })).await;
    assert_eq!(looks(t.ok(&s, "tree", json!({})).await["data"].clone())[1], (json!("学习"), Value::Null, Value::Null));
    for bad in [json!({ "folder": "/学习" }), json!({ "folder": "/学习", "color": "black" }), json!({ "folder": "/", "icon": "book" })]
    {
        assert_eq!(t.tool(&s, "style", bad.clone()).await.status, StatusCode::BAD_REQUEST, "{bad}");
    }
}
