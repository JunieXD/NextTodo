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
    assert_eq!(tree[2]["pending"], 1);

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
    assert_eq!(due, json!({ "date": "2026-11-21", "time": "09:00", "remind_before": 30 }), "moving the deadline keeps the reminder");
    t.ok(&s, "edit", json!({ "task": 1, "remind": null })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"]["remind_before"], Value::Null);
    assert_eq!(t.tool(&s, "edit", json!({ "task": 1 })).await.status, StatusCode::BAD_REQUEST, "nothing to change");
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
    assert_eq!(failed["result"]["content"][0]["text"], "不存在事项 #9");
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
}
