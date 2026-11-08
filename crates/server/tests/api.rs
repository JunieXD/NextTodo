mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{TestApp, id};
use nexttodo_server::{MUTATION_RETENTION_MS, db};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn login_sessions_and_logout() {
    let t = TestApp::new();
    let admin = t.admin().await;
    assert_eq!(t.login("alice", "wrong password").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.login("nobody", "correct horse").await.status, StatusCode::UNAUTHORIZED);

    let login = t.login("ALICE", "correct horse").await;
    assert_eq!(login.status, StatusCode::OK, "usernames are case-insensitive");
    let cookie = login.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("nt_session=nt_") && cookie.contains("HttpOnly") && cookie.contains("Secure"));

    let me = t.call("GET", "/me", Some(&admin), None).await;
    assert_eq!(me.body["username"], "alice");
    assert_eq!(me.body["is_admin"], true);

    let via_cookie =
        Request::builder().uri("/api/v1/me").header(header::COOKIE, cookie.split(';').next().unwrap()).body(Body::empty()).unwrap();
    assert_eq!(t.app.clone().oneshot(via_cookie).await.unwrap().status(), StatusCode::OK);

    assert_eq!(t.call("POST", "/auth/logout", Some(&admin), None).await.status, StatusCode::NO_CONTENT);
    let after = t.call("GET", "/me", Some(&admin), None).await;
    assert_eq!((after.status, after.body["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("unauthenticated")));
}

#[tokio::test]
async fn invites_registration_and_token_permissions() {
    let t = TestApp::new();
    let admin = t.admin().await;
    let code = t.call("POST", "/invites", Some(&admin), None).await.body["code"].as_str().unwrap().to_owned();
    let reg = json!({ "invite": code, "username": "friend", "password": "password123", "display_name": "朋友", "device": "手机" });
    let first = t.call("POST", "/auth/register", None, Some(reg.clone())).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(t.call("POST", "/auth/register", None, Some(reg)).await.status, StatusCode::CONFLICT, "an invite works once");
    let invites = t.call("GET", "/invites", Some(&admin), None).await.body;
    assert_eq!(invites[0]["used_by"], "friend");

    let friend = first.body["secret"].as_str().unwrap();
    assert_eq!(t.call("POST", "/invites", Some(friend), None).await.status, StatusCode::FORBIDDEN, "only admins invite");

    let made = t.call("POST", "/tokens", Some(&admin), Some(json!({ "name": "Claude Code", "scope": "read" }))).await.body;
    let read_only = made["secret"].as_str().unwrap();
    assert_eq!(t.call("GET", "/snapshot", Some(read_only), None).await.status, StatusCode::OK);
    let denied = t.mutate(read_only, json!([{ "op": "folder.create", "id": id(), "name": "工作" }])).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let minted = t.call("POST", "/tokens", Some(read_only), Some(json!({ "name": "x", "scope": "write" }))).await;
    assert_eq!(minted.status, StatusCode::FORBIDDEN, "API tokens cannot mint tokens");
    assert_eq!(t.call("POST", "/auth/logout", Some(read_only), None).await.status, StatusCode::FORBIDDEN, "nor revoke themselves");

    let tokens = t.call("GET", "/tokens", Some(&admin), None).await.body;
    assert_eq!(tokens.as_array().unwrap().len(), 2);
    let token_id = made["token"]["id"].as_str().unwrap();
    assert_eq!(t.call("DELETE", &format!("/tokens/{token_id}"), Some(&admin), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(t.call("GET", "/snapshot", Some(read_only), None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn one_mutation_creates_folders_tasks_and_dependencies() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (folder, a, b) = (id(), id(), id());
    let r = t
        .mutate(
            &s,
            json!([
                { "op": "folder.create", "id": folder, "name": "毕业论文" },
                { "op": "task.create", "id": a, "folder_id": folder, "title": "设计问卷", "due": { "date": "2026-11-02", "time": "18:00", "remind_before": 60 } },
                { "op": "task.create", "id": b, "folder_id": folder, "title": "发放问卷" },
                { "op": "dep.add", "before": a, "after": b }
            ]),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["effects"].as_array().unwrap().len(), 4);

    let snap = t.snapshot(&s).await;
    assert_eq!(snap["seq"], r.body["seq"]);
    let tasks = snap["tasks"].as_array().unwrap();
    assert_eq!((tasks[0]["number"].as_i64(), tasks[1]["number"].as_i64()), (Some(1), Some(2)));
    assert_eq!((tasks[0]["sort_key"].as_str(), tasks[1]["sort_key"].as_str()), (Some("a0"), Some("a1")));
    assert_eq!(tasks[0]["due"], json!({ "date": "2026-11-02", "time": "18:00", "remind_before": 60 }));
    assert_eq!(tasks[0]["created_via"], Value::Null);
    assert_eq!(snap["dependencies"], json!([{ "before": a, "after": b, "created_at": tasks[0]["created_at"] }]));
}

#[tokio::test]
async fn cycles_are_rejected_and_nothing_is_applied() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (a, b, c) = (id(), id(), id());
    t.mutate(
        &s,
        json!([
            { "op": "task.create", "id": a, "title": "A" },
            { "op": "task.create", "id": b, "title": "B" },
            { "op": "task.create", "id": c, "title": "C" },
            { "op": "dep.add", "before": a, "after": b },
            { "op": "dep.add", "before": b, "after": c }
        ]),
    )
    .await;
    let before = t.snapshot(&s).await;
    let r = t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "D" }, { "op": "dep.add", "before": c, "after": a }])).await;
    assert_eq!((r.status, r.body["code"].as_str()), (StatusCode::CONFLICT, Some("cycle")));
    assert_eq!(r.body["details"]["path"], json!([c, a, b, c]));
    assert_eq!(t.snapshot(&s).await, before, "the whole mutation is rolled back");

    let dup = t.mutate(&s, json!([{ "op": "dep.add", "before": a, "after": b }])).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    let own = t.mutate(&s, json!([{ "op": "dep.add", "before": a, "after": a }])).await;
    assert_eq!(own.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn folders_cannot_move_into_their_own_subtree() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (top, mid, leaf) = (id(), id(), id());
    t.mutate(
        &s,
        json!([
            { "op": "folder.create", "id": top, "name": "学习" },
            { "op": "folder.create", "id": mid, "name": "毕业论文", "parent_id": top },
            { "op": "folder.create", "id": leaf, "name": "答辩", "parent_id": mid }
        ]),
    )
    .await;
    let r = t.mutate(&s, json!([{ "op": "folder.update", "id": top, "parent_id": leaf }])).await;
    assert_eq!(r.body["code"], "folder_cycle");
    let to_root = t.mutate(&s, json!([{ "op": "folder.update", "id": leaf, "parent_id": null }])).await;
    assert_eq!(to_root.status, StatusCode::OK);
    let folders = t.snapshot(&s).await["folders"].clone();
    assert!(folders.as_array().unwrap().iter().any(|f| f["id"] == leaf.as_str() && f["parent_id"].is_null()));
}

#[tokio::test]
async fn deleting_a_folder_can_be_undone_completely() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (top, sub, a, b, outside) = (id(), id(), id(), id(), id());
    t.mutate(
        &s,
        json!([
            { "op": "folder.create", "id": top, "name": "毕业论文" },
            { "op": "folder.create", "id": sub, "name": "答辩", "parent_id": top },
            { "op": "task.create", "id": a, "folder_id": top, "title": "写初稿", "description": "- [ ] 引言" },
            { "op": "task.create", "id": b, "folder_id": sub, "title": "做 PPT" },
            { "op": "task.create", "id": outside, "title": "交电费" },
            { "op": "dep.add", "before": a, "after": b },
            { "op": "dep.add", "before": outside, "after": a }
        ]),
    )
    .await;
    let before = t.snapshot(&s).await;
    let delete_id = id();
    let deleted =
        t.call("POST", "/mutations", Some(&s), Some(json!({ "id": delete_id, "ops": [{ "op": "folder.delete", "id": top }] }))).await;
    assert_eq!(deleted.body["effects"].as_array().unwrap().len(), 6, "2 dependencies, 2 tasks, 2 folders");
    let after = t.snapshot(&s).await;
    assert_eq!((after["folders"].as_array().unwrap().len(), after["tasks"].as_array().unwrap().len()), (0, 1));
    assert_eq!(after["dependencies"], json!([]));

    t.advance(1000);
    let undo = t.mutate(&s, json!([{ "op": "undo", "mutation": delete_id }])).await;
    assert_eq!(undo.status, StatusCode::OK, "{}", undo.body);
    let restored = t.snapshot(&s).await;
    let strip = |v: &Value| {
        let mut v = v.clone();
        for list in ["folders", "tasks"] {
            for e in v[list].as_array_mut().unwrap() {
                e.as_object_mut().unwrap().remove("updated_at");
            }
        }
        v.as_object_mut().unwrap().remove("seq");
        v
    };
    assert_eq!(strip(&restored), strip(&before), "same ids, numbers, content and dependencies");

    let again = t.mutate(&s, json!([{ "op": "undo", "mutation": delete_id }])).await;
    assert_eq!(again.body["effects"], json!([]), "undoing twice changes nothing more");
}

#[tokio::test]
async fn updates_completion_and_undo_of_an_update() {
    let t = TestApp::new();
    let s = t.admin().await;
    let a = id();
    t.mutate(&s, json!([{ "op": "task.create", "id": a, "title": "设计问卷", "due": { "date": "2026-11-20" } }])).await;
    t.advance(5000);
    let update_id = id();
    let ops = json!([{ "op": "task.update", "id": a, "title": "设计问卷（第二版）", "done": true, "due": null }]);
    let r = t.call("POST", "/mutations", Some(&s), Some(json!({ "id": update_id, "ops": ops }))).await;
    let task = &r.body["effects"][0]["after"];
    assert_eq!((task["title"].as_str(), task["due"].is_null()), (Some("设计问卷（第二版）"), true));
    assert_eq!(task["done_at"], t.state.clock.now());

    let same = t.mutate(&s, json!([{ "op": "task.update", "id": a, "done": true }])).await;
    assert_eq!(same.body["effects"], json!([]), "completing a completed task changes nothing");

    t.mutate(&s, json!([{ "op": "undo", "mutation": update_id }])).await;
    let task = &t.snapshot(&s).await["tasks"][0];
    assert_eq!((task["title"].as_str(), task["done_at"].is_null()), (Some("设计问卷"), true));
    assert_eq!(task["due"], json!({ "date": "2026-11-20" }));
}

#[tokio::test]
async fn resubmitting_a_mutation_applies_it_once() {
    let t = TestApp::new();
    let s = t.admin().await;
    let body = json!({ "id": id(), "ops": [{ "op": "task.create", "id": id(), "title": "交电费" }] });
    let first = t.call("POST", "/mutations", Some(&s), Some(body.clone())).await;
    let second = t.call("POST", "/mutations", Some(&s), Some(body)).await;
    assert_eq!(first.body, second.body);
    assert_eq!(t.snapshot(&s).await["tasks"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn changes_since_and_resync_after_pruning() {
    let t = TestApp::new();
    let s = t.admin().await;
    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "Claude Code", "scope": "write" }))).await.body;
    let agent = token["secret"].as_str().unwrap();
    let first = t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "一" }])).await.body["seq"].as_i64().unwrap();
    t.mutate(agent, json!([{ "op": "task.create", "id": id(), "title": "二" }])).await;

    let all = t.call("GET", "/changes?since=0", Some(&s), None).await.body;
    assert_eq!(all["changes"].as_array().unwrap().len(), 2);
    assert_eq!((all["changes"][0]["actor"].clone(), all["changes"][1]["actor"].as_str()), (Value::Null, Some("Claude Code")));
    assert_eq!(all["changes"][1]["effects"][0]["after"]["created_via"], "Claude Code");
    let newer = t.call("GET", &format!("/changes?since={first}"), Some(&s), None).await.body;
    assert_eq!(newer["changes"].as_array().unwrap().len(), 1);
    assert_eq!(newer["seq"], all["seq"]);

    t.advance(MUTATION_RETENTION_MS + 1);
    let cutoff = t.state.clock.now() - MUTATION_RETENTION_MS;
    t.state.db.call(move |c| Ok(db::prune(c, cutoff)?)).await.unwrap();
    let stale = t.call("GET", "/changes?since=0", Some(&s), None).await;
    assert_eq!((stale.status, stale.body["code"].as_str()), (StatusCode::GONE, Some("resync")));
    let current = t.call("GET", &format!("/changes?since={}", all["seq"]), Some(&s), None).await;
    assert_eq!((current.status, current.body["changes"].clone()), (StatusCode::OK, json!([])));
    assert_eq!(t.snapshot(&s).await["seq"], all["seq"], "the latest seq survives pruning");
}

#[tokio::test]
async fn old_mutations_are_recognised_after_pruning() {
    let t = TestApp::new();
    let s = t.admin().await;
    let a = id();
    t.mutate(&s, json!([{ "op": "task.create", "id": a, "title": "初稿" }])).await;
    let old = json!({ "id": id(), "ops": [{ "op": "task.update", "id": a, "title": "旧编辑" }] });
    let first = t.call("POST", "/mutations", Some(&s), Some(old.clone())).await.body;

    t.advance(MUTATION_RETENTION_MS + 1);
    let cutoff = t.state.clock.now() - MUTATION_RETENTION_MS;
    t.state.db.call(move |c| Ok(db::prune(c, cutoff)?)).await.unwrap();
    t.mutate(&s, json!([{ "op": "task.update", "id": a, "title": "最新编辑" }])).await;

    let again = t.call("POST", "/mutations", Some(&s), Some(old)).await;
    assert_eq!((again.status, again.body["seq"].clone(), again.body["effects"].clone()), (StatusCode::OK, first["seq"].clone(), json!([])));
    assert_eq!(t.snapshot(&s).await["tasks"][0]["title"], "最新编辑", "the old edit is not applied again");
}

#[tokio::test]
async fn the_log_is_cut_at_a_seq_that_only_moves_forward() {
    let t = TestApp::new();
    let s = t.admin().await;
    // The clock goes back between the two, so the later mutation is the older one by time.
    let first = t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "一" }])).await.body["seq"].as_i64().unwrap();
    t.advance(-10);
    t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "二" }])).await;
    let prune = |cutoff: i64| t.state.db.call(move |c| Ok(db::prune(c, cutoff)?));

    let now = t.state.clock.now();
    prune(now + 5).await.unwrap();
    let r = t.call("GET", &format!("/changes?since={first}"), Some(&s), None).await;
    assert_eq!(r.status, StatusCode::GONE, "everything up to the newest pruned mutation is gone");
    prune(now + 11).await.unwrap();
    let r = t.call("GET", &format!("/changes?since={first}"), Some(&s), None).await;
    assert_eq!(r.status, StatusCode::GONE, "pruning the first one later does not move the cut back");
}

#[tokio::test]
async fn deep_folder_trees_can_be_deleted() {
    let t = TestApp::new();
    let s = t.admin().await;
    let ids: Vec<String> = (0..1001).map(|_| id()).collect();
    let create = |i: usize| json!({ "op": "folder.create", "id": ids[i], "name": format!("第 {i} 层"), "parent_id": (i > 0).then(|| ids[i - 1].clone()) });
    t.mutate(&s, (0..1000).map(create).collect()).await;
    t.mutate(&s, json!([create(1000), { "op": "task.create", "id": id(), "title": "最深处", "folder_id": ids[1000] }])).await;

    let r = t.mutate(&s, json!([{ "op": "folder.delete", "id": ids[0] }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let snap = t.snapshot(&s).await;
    assert_eq!((snap["folders"].clone(), snap["tasks"].clone()), (json!([]), json!([])));
}

#[tokio::test]
async fn undoing_an_update_keeps_what_the_update_did_not_touch() {
    let t = TestApp::new();
    let s = t.admin().await;
    let a = id();
    t.mutate(&s, json!([{ "op": "task.create", "id": a, "title": "初稿" }])).await;
    let update = id();
    t.call("POST", "/mutations", Some(&s), Some(json!({ "id": update, "ops": [{ "op": "task.update", "id": a, "title": "二稿" }] }))).await;
    t.mutate(&s, json!([{ "op": "task.delete", "id": a }])).await;
    t.mutate(&s, json!([{ "op": "task.create", "id": a, "title": "重建" }])).await;

    t.mutate(&s, json!([{ "op": "task.update", "id": a, "description": "后来写的", "done": true }])).await;

    let undo = t.mutate(&s, json!([{ "op": "undo", "mutation": update }])).await.body;
    let task = t.snapshot(&s).await["tasks"][0].clone();
    assert_eq!((task["title"].as_str(), task["number"].as_i64()), (Some("初稿"), Some(2)));
    assert_eq!((task["description"].as_str(), task["done_at"].is_null()), (Some("后来写的"), false), "later edits stay");
    assert_eq!(undo["effects"][0]["after"], task, "the recorded change matches the stored task");

    let (top, other) = (id(), id());
    t.mutate(&s, json!([{ "op": "folder.create", "id": top, "name": "旧名" }, { "op": "folder.create", "id": other, "name": "别处" }]))
        .await;
    let rename = id();
    t.call("POST", "/mutations", Some(&s), Some(json!({ "id": rename, "ops": [{ "op": "folder.update", "id": top, "name": "新名" }] })))
        .await;
    t.mutate(&s, json!([{ "op": "folder.update", "id": top, "parent_id": other }])).await;
    t.mutate(&s, json!([{ "op": "undo", "mutation": rename }])).await;
    let folders = t.snapshot(&s).await["folders"].clone();
    let folder = folders.as_array().unwrap().iter().find(|f| f["id"] == top.as_str()).unwrap();
    assert_eq!((folder["name"].as_str(), folder["parent_id"].as_str()), (Some("旧名"), Some(other.as_str())));
}

#[tokio::test]
async fn users_cannot_see_or_touch_each_others_data() {
    let t = TestApp::new();
    let admin = t.admin().await;
    let friend = t.user(&admin, "friend").await;
    let (task, folder) = (id(), id());
    t.mutate(
        &admin,
        json!([{ "op": "folder.create", "id": folder, "name": "私人" }, { "op": "task.create", "id": task, "title": "秘密" }]),
    )
    .await;

    let snap = t.snapshot(&friend).await;
    assert_eq!((snap["tasks"].clone(), snap["folders"].clone()), (json!([]), json!([])));
    for ops in [
        json!([{ "op": "task.update", "id": task, "title": "改掉" }]),
        json!([{ "op": "task.create", "id": id(), "title": "x", "folder_id": folder }]),
        json!([{ "op": "task.create", "id": id(), "title": "x" }, { "op": "dep.add", "before": task, "after": task }]),
    ] {
        let r = t.mutate(&friend, ops).await;
        assert!(matches!(r.status, StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST), "{}", r.body);
    }
    let reuse = t.mutate(&friend, json!([{ "op": "task.create", "id": task, "title": "抢 ID" }])).await;
    assert_eq!(reuse.status, StatusCode::CONFLICT);
    assert_eq!(t.snapshot(&admin).await["tasks"][0]["title"], "秘密");
}

#[tokio::test]
async fn done_moves_a_repeating_task_on_by_the_day_on_the_device() {
    let t = TestApp::new();
    let s = t.admin().await;
    let task = id();
    let due = json!({ "date": "2026-01-31", "repeat": { "every": 1, "unit": "month", "from": "due" } });
    t.mutate(&s, json!([{ "op": "task.create", "id": task, "title": "交房租", "due": due }])).await;
    let r = t.mutate(&s, json!([{ "op": "task.update", "id": task, "done": true }])).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("today")));
    let r = t.mutate(&s, json!([{ "op": "task.update", "id": task, "done": true, "today": "2026-01-31" }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let after = &r.body["effects"][0]["after"];
    assert_eq!((after["done_at"].clone(), after["due"]["date"].clone()), (Value::Null, json!("2026-02-28")), "the end of the month");
}

#[tokio::test]
async fn bad_input_names_the_field() {
    let t = TestApp::new();
    let s = t.admin().await;
    let r = t
        .mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "x", "due": { "date": "2026-11-20", "remind_before": 10 } }]))
        .await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("ops[0]")));
    assert_eq!(r.body["message"], "设置提醒需要具体的截止时间");
    let r = t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "   " }])).await;
    assert_eq!((r.body["details"]["field"].as_str(), r.body["message"].as_str()), (Some("ops[0]"), Some("标题不能为空")));
    let r = t.call("GET", "/changes?since=abc", Some(&s), None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = t.call("GET", "/nope", Some(&s), None).await;
    assert_eq!(r.body["code"], "not_found");

    let raw = |content_type: &str, body: &str| {
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let login = r#"{"username":"alice","password":"correct horse","device":"x"}"#;
    let form = t.app.clone().oneshot(raw("text/plain", login)).await.unwrap();
    assert_eq!(form.status(), StatusCode::BAD_REQUEST, "a cross-site form cannot log anyone in");
    let trailing = t.app.clone().oneshot(raw("application/json", &format!("{login} garbage"))).await.unwrap();
    assert_eq!(trailing.status(), StatusCode::BAD_REQUEST);
    let ok = t.app.clone().oneshot(raw("application/json; charset=utf-8", login)).await.unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
}

#[tokio::test]
async fn changing_the_password_signs_out_other_devices() {
    let t = TestApp::new();
    let here = t.admin().await;
    let phone = t.login("alice", "correct horse").await.body["secret"].as_str().unwrap().to_owned();
    let token = t.call("POST", "/tokens", Some(&here), Some(json!({ "name": "Claude Code", "scope": "write" }))).await.body;
    let agent = token["secret"].as_str().unwrap();

    let wrong =
        t.call("POST", "/auth/password", Some(&here), Some(json!({ "old_password": "nope", "new_password": "new password" }))).await;
    assert_eq!((wrong.status, wrong.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("old_password")));
    let by_agent = t
        .call("POST", "/auth/password", Some(agent), Some(json!({ "old_password": "correct horse", "new_password": "agent password" })))
        .await;
    assert_eq!(by_agent.status, StatusCode::FORBIDDEN);
    let ok = t
        .call("POST", "/auth/password", Some(&here), Some(json!({ "old_password": "correct horse", "new_password": "new password" })))
        .await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);

    assert_eq!(t.call("GET", "/me", Some(&here), None).await.status, StatusCode::OK, "this device stays signed in");
    assert_eq!(t.call("GET", "/me", Some(&phone), None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.call("GET", "/me", Some(agent), None).await.status, StatusCode::OK, "API tokens keep working");
    assert_eq!(t.login("alice", "correct horse").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.login("alice", "new password").await.status, StatusCode::OK);

    let renamed = t.call("PATCH", "/me", Some(&here), Some(json!({ "display_name": "小 J" }))).await;
    assert_eq!(renamed.body["display_name"], "小 J");
}
