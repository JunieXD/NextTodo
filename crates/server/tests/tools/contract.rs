//! What every tool keeps to: reads as of a change, retries, permissions and arguments, MCP, and what outputs leave out.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::common::{self, TestApp};
use crate::{numbers, text};

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
