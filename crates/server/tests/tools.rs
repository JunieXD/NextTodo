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
    assert_eq!(design["due"], json!({ "date": "2026-11-20", "time": "18:00", "remind_before": 60 }));

    t.ok(&s, "undo", json!({ "batch": applied["data"]["batch"] })).await;
    let snap = t.snapshot(&s).await;
    assert_eq!(snap["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(snap["folders"].as_array().unwrap().len(), 1, "the folders the plan made are gone too");
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
    assert_eq!(numbers(&t.ok(&s, "next", json!({})).await["data"]), vec![3, 1], "tasks with a due date come first");

    let done = t.ok(&s, "done", json!({ "tasks": [1] })).await;
    assert_eq!(numbers(&done["data"]["unlocked"]), vec![2]);
    assert!(text(&done).contains("解锁 #2 改稿"), "{}", text(&done));
    assert_eq!(numbers(&t.ok(&s, "next", json!({})).await["data"]), vec![3, 2]);

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
    assert_eq!(tree[2]["pending"], 1);

    t.ok(&s, "delete", json!({ "tasks": [1], "folders": ["/生活", "/生活/毕业论文", "/生活"] })).await;
    let snap = t.snapshot(&s).await;
    assert_eq!((snap["folders"].as_array().unwrap().len(), snap["tasks"].clone()), (1, json!([])));
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
    assert_eq!(t.snapshot(&s).await["tasks"].as_array().unwrap().len(), 1);

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

    let (_, list) = rpc(Some(&s), "tools/list", json!({})).await;
    let next = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "next").unwrap().clone();
    assert_eq!(next["annotations"]["readOnlyHint"], true);

    let (_, called) = rpc(Some(&s), "tools/call", json!({ "name": "next", "arguments": {} })).await;
    assert_eq!(called["result"]["content"][0]["text"], "#1 交电费");
    let (_, failed) = rpc(Some(&s), "tools/call", json!({ "name": "show", "arguments": { "task": 9 } })).await;
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(failed["result"]["content"][0]["text"], "没有 #9 这个事项");
}
