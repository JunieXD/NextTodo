//! Plans applied as one batch, or refused whole.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{numbers, text};
use crate::common::TestApp;

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
