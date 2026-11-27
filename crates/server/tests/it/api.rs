use crate::common::{TestApp, id};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use nexttodo_server::{MUTATION_RETENTION_MS, auth, db};
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

/// A page on plain HTTP, from a server used without HTTPS: its browser keeps no `Secure` cookie, so
/// it gets one without, to sign in and to sign out with.
#[tokio::test]
async fn a_page_on_plain_http_gets_a_cookie_it_can_keep() {
    let t = TestApp::new();
    t.admin().await;
    let sign_in = |origin: &'static str| {
        let req = Request::post("/api/v1/auth/login").header(header::CONTENT_TYPE, "application/json").header(header::ORIGIN, origin);
        t.raw(req, Body::from(json!({ "username": "alice", "password": "correct horse", "device": "Chrome · macOS" }).to_string()))
    };
    let (_, headers, _) = sign_in("https://todo.example.com").await;
    assert!(headers[header::SET_COOKIE].to_str().unwrap().contains("Secure"));
    let (status, headers, _) = sign_in("http://192.168.1.10:8080").await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("nt_session=nt_") && !cookie.contains("Secure"), "{cookie}");

    let out = Request::post("/api/v1/auth/logout")
        .header(header::COOKIE, cookie.split(';').next().unwrap())
        .header(header::ORIGIN, "http://192.168.1.10:8080");
    let (status, headers, _) = t.raw(out, Body::empty()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cleared = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cleared.starts_with("nt_session=;") && cleared.contains("Max-Age=0") && !cleared.contains("Secure"), "{cleared}");
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
                { "op": "task.create", "id": a, "folder_id": folder, "title": "设计问卷", "due": { "date": "2026-11-02", "time": "18:00", "reminders": [{ "before": 60 }] } },
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
    assert_eq!(tasks[0]["due"], json!({ "date": "2026-11-02", "time": "18:00", "reminders": [{ "before": 60 }] }));
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
async fn a_time_done_is_not_taken_back_by_a_change_that_does_it_again_first() {
    let t = TestApp::new();
    let s = t.admin().await;
    let task = id();
    let due = json!({ "date": "2026-11-10", "repeat": { "every": 1, "unit": "day", "from": "due" } });
    t.mutate(&s, json!([{ "op": "task.create", "id": task, "title": "背单词", "due": due }])).await;
    let first = id();
    let done = json!({ "op": "task.update", "id": task, "done": true, "today": "2026-11-10" });
    let r = t.call("POST", "/mutations", Some(&s), Some(json!({ "id": first, "ops": [done] }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = t.mutate(&s, json!([done, { "op": "undo", "mutation": first }])).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    let snap = t.snapshot(&s).await;
    assert_eq!(snap["tasks"][0]["completions"], json!({ "count": 1, "last": "2026-11-10" }), "nothing of it made");
}

#[tokio::test]
async fn a_change_that_did_a_task_twice_is_undone_whole() {
    let t = TestApp::new();
    let s = t.admin().await;
    let task = id();
    let due = json!({ "date": "2026-11-10", "repeat": { "every": 1, "unit": "day", "from": "due" } });
    t.mutate(&s, json!([{ "op": "task.create", "id": task, "title": "背单词", "due": due }])).await;
    let both = id();
    let done = |today| json!({ "op": "task.update", "id": task, "done": true, "today": today });
    let r = t.call("POST", "/mutations", Some(&s), Some(json!({ "id": both, "ops": [done("2026-11-10"), done("2026-11-11")] }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = t.mutate(&s, json!([{ "op": "undo", "mutation": both }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let snap = t.snapshot(&s).await;
    assert_eq!((&snap["tasks"][0]["completions"], &snap["tasks"][0]["due"]), (&Value::Null, &due));
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

    // Due after the last day it repeats on is refused as given, even done at once, which would take
    // the repeat away with it.
    let past = json!({ "date": "2026-03-31", "repeat": { "every": 1, "unit": "month", "from": "due", "until": "2026-03-30" } });
    let r = t.mutate(&s, json!([{ "op": "task.update", "id": task, "due": past, "done": true, "today": "2026-02-28" }])).await;
    assert_eq!((r.status, r.body["message"].as_str()), (StatusCode::BAD_REQUEST, Some("重复到不能早于截止日期")), "{}", r.body);
}

#[tokio::test]
async fn done_from_a_list_behind_does_not_do_the_next_time_too() {
    let t = TestApp::new();
    let s = t.admin().await;
    let task = id();
    let daily = json!({ "date": "2026-11-20", "repeat": { "every": 1, "unit": "day", "from": "due" } });
    t.mutate(&s, json!([{ "op": "task.create", "id": task, "title": "浇花", "due": daily }])).await;
    // Done on the computer; then from the phone's notification of the same time.
    let done = |due: &str, completions: u32| json!([{ "op": "task.update", "id": task, "done": true, "today": "2026-11-20", "occurrence": { "due": due, "completions": completions } }]);
    t.mutate(&s, done("2026-11-20", 0)).await;
    let r = t.mutate(&s, done("2026-11-20", 0)).await;
    assert_eq!((r.status, r.body["effects"].clone()), (StatusCode::OK, json!([])), "{}", r.body);
    let r = t.mutate(&s, done("2026-11-21", 1)).await;
    assert_eq!(r.body["effects"][0]["after"]["due"]["date"], "2026-11-22", "{}", r.body);

    // From the day it is done, done a day early: the next time is due on the same day.
    let early = id();
    let daily = json!({ "date": "2026-11-21", "repeat": { "every": 1, "unit": "day", "from": "done" } });
    t.mutate(&s, json!([{ "op": "task.create", "id": early, "title": "吃药", "due": daily }])).await;
    let done = |completions: u32| json!([{ "op": "task.update", "id": early, "done": true, "today": "2026-11-20", "occurrence": { "due": "2026-11-21", "completions": completions } }]);
    let r = t.mutate(&s, done(0)).await;
    assert_eq!(r.body["effects"][0]["after"]["due"]["date"], "2026-11-21", "{}", r.body);
    let r = t.mutate(&s, done(0)).await;
    assert_eq!(r.body["effects"], json!([]), "{}", r.body);
    let r = t.mutate(&s, done(1)).await;
    assert_eq!(r.body["effects"][0]["after"]["completions"]["count"], 2, "{}", r.body);
}

#[tokio::test]
async fn bad_input_names_the_field() {
    let t = TestApp::new();
    let s = t.admin().await;
    let r = t
        .mutate(
            &s,
            json!([{ "op": "task.create", "id": id(), "title": "x", "due": { "date": "2026-11-20", "reminders": [{ "before": 10 }] } }]),
        )
        .await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("ops[0]")));
    assert_eq!(r.body["message"], "提前多久的提醒需要具体的截止时间");
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

#[tokio::test]
async fn a_device_checks_the_password_without_signing_in_again() {
    let t = TestApp::new();
    let here = t.admin().await;
    let token = t.call("POST", "/tokens", Some(&here), Some(json!({ "name": "Claude Code", "scope": "write" }))).await.body;
    let agent = token["secret"].as_str().unwrap();

    let right = t.call("POST", "/auth/verify", Some(&here), Some(json!({ "password": "correct horse" }))).await;
    assert_eq!(right.status, StatusCode::NO_CONTENT);
    let wrong = t.call("POST", "/auth/verify", Some(&here), Some(json!({ "password": "nope" }))).await;
    assert_eq!((wrong.status, wrong.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("password")));
    let by_agent = t.call("POST", "/auth/verify", Some(agent), Some(json!({ "password": "correct horse" }))).await;
    assert_eq!(by_agent.status, StatusCode::FORBIDDEN);
    let devices = t.call("GET", "/tokens", Some(&here), None).await.body;
    assert_eq!(devices.as_array().unwrap().iter().filter(|d| d["kind"] == "session").count(), 1, "{devices}");
}

#[tokio::test]
async fn resetting_a_forgotten_password_signs_out_every_device() {
    let t = TestApp::new();
    let here = t.admin().await;
    let token = t.call("POST", "/tokens", Some(&here), Some(json!({ "name": "Claude Code", "scope": "write" }))).await.body;
    let agent = token["secret"].as_str().unwrap();

    assert_eq!(auth::user_named(&t.state, "nobody".into()).await.unwrap_err().code(), "not_found");
    let alice = auth::user_named(&t.state, "ALICE".into()).await.unwrap();
    assert_eq!(auth::reset_password(&t.state, alice.clone(), "short".into()).await.unwrap_err().code(), "invalid");
    assert_eq!(t.login("alice", "correct horse").await.status, StatusCode::OK, "nothing changed");
    auth::reset_password(&t.state, alice, "new password".into()).await.unwrap();

    assert_eq!(t.call("GET", "/me", Some(&here), None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.call("GET", "/me", Some(agent), None).await.status, StatusCode::OK, "API tokens keep working");
    assert_eq!(t.login("alice", "correct horse").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.login("alice", "new password").await.status, StatusCode::OK);
}

/// A change made on a device on `today`, and its id.
fn made_on(today: &str, ops: Value) -> (String, Value) {
    let m = id();
    (m.clone(), json!({ "id": m, "today": today, "ops": ops }))
}

/// Task `id` as `effects` left it.
fn task_in<'a>(effects: &'a Value, id: &str) -> &'a Value {
    effects.as_array().unwrap().iter().rev().find(|e| e["kind"] == "task" && e["after"]["id"] == id).map(|e| &e["after"]).unwrap()
}

#[tokio::test]
async fn a_due_date_after_the_prerequisites_comes_when_they_are_done_and_goes_when_one_is_reopened() {
    let t = TestApp::new();
    let s = t.admin().await;
    let post = |body: Value| t.call("POST", "/mutations", Some(&s), Some(body));
    let (invoice, claim) = (id(), id());
    let due_after = json!({ "days": 30, "time": "18:00", "reminders": [{ "days": 1, "time": "09:00" }] });
    let (_, made) = made_on(
        "2026-11-20",
        json!([
            { "op": "task.create", "id": invoice, "title": "拿到发票" },
            { "op": "task.create", "id": claim, "title": "报销", "due_after": due_after },
            { "op": "dep.add", "before": invoice, "after": claim },
        ]),
    );
    let r = post(made).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let waiting = task_in(&r.body["effects"], &claim);
    assert_eq!((&waiting["due_after"], &waiting["due"], &waiting.get("unlocked_on")), (&due_after, &Value::Null, &None));

    let ops = json!([{ "op": "task.update", "id": invoice, "done": true }]);
    let r = post(json!({ "id": id(), "today": "2026-11-24", "time": "23:30", "ops": ops })).await;
    let claimed = task_in(&r.body["effects"], &claim);
    assert_eq!(claimed["unlocked_on"], "2026-11-24", "{}", r.body);
    assert_eq!(claimed["due"], json!({ "date": "2026-12-24", "time": "18:00", "reminders": [{ "days": 1, "time": "09:00" }] }));

    let r = t.mutate(&s, json!([{ "op": "task.update", "id": invoice, "done": false }])).await;
    assert_eq!(task_in(&r.body["effects"], &claim)["due"], Value::Null, "waiting again: {}", r.body);
    // Done again a day later, it counts from then; undoing that takes its date back with it.
    let (again, body) = made_on("2026-11-25", json!([{ "op": "task.update", "id": invoice, "done": true }]));
    let r = post(body).await;
    assert_eq!(task_in(&r.body["effects"], &claim)["due"]["date"], "2026-12-25", "{}", r.body);
    let r = t.mutate(&s, json!([{ "op": "undo", "mutation": again }])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let snap = t.snapshot(&s).await;
    let back = snap["tasks"].as_array().unwrap().iter().find(|x| x["id"] == claim.as_str()).unwrap();
    assert_eq!((&back["due"], back.get("unlocked_on"), &back["due_after"]), (&Value::Null, None, &due_after));
}

#[tokio::test]
async fn a_due_date_after_the_prerequisites_counts_from_a_later_start_and_from_the_last_one_taken_away() {
    let t = TestApp::new();
    let s = t.admin().await;
    let post = |body: Value| t.call("POST", "/mutations", Some(&s), Some(body));
    let (report, visit) = (id(), id());
    let (_, body) = made_on(
        "2026-11-20",
        json!([
            { "op": "task.create", "id": report, "title": "拿到报告" },
            { "op": "task.create", "id": visit, "title": "复诊", "start": "2026-12-01", "due_after": { "days": 7 } },
            { "op": "dep.add", "before": report, "after": visit },
        ]),
    );
    post(body).await;
    let (_, body) = made_on("2026-11-21", json!([{ "op": "dep.remove", "before": report, "after": visit }]));
    let r = post(body).await;
    let v = task_in(&r.body["effects"], &visit);
    assert_eq!((&v["unlocked_on"], &v["due"]["date"]), (&json!("2026-11-21"), &json!("2026-12-08")), "from its start: {}", r.body);

    // Linked to a prerequisite done, it stays; to one not done, it waits again.
    let done = id();
    let (_, body) = made_on(
        "2026-11-22",
        json!([
            { "op": "task.create", "id": done, "title": "挂号", "done": true },
            { "op": "dep.add", "before": done, "after": visit },
        ]),
    );
    let r = post(body).await;
    assert!(r.body["effects"].as_array().unwrap().iter().all(|e| e["after"]["id"] != visit.as_str()), "{}", r.body);
    let r = t.mutate(&s, json!([{ "op": "dep.add", "before": report, "after": visit }])).await;
    assert_eq!(task_in(&r.body["effects"], &visit)["due"], Value::Null, "{}", r.body);

    // Its start moved on moves the date it was given; deleting the last prerequisite gives it one.
    let (_, body) = made_on("2026-11-23", json!([{ "op": "task.delete", "id": report }]));
    let r = post(body).await;
    assert_eq!(task_in(&r.body["effects"], &visit)["due"]["date"], "2026-12-08", "{}", r.body);
    let r = t.mutate(&s, json!([{ "op": "task.update", "id": visit, "start": "2026-12-10" }])).await;
    assert_eq!(task_in(&r.body["effects"], &visit)["due"]["date"], "2026-12-17", "{}", r.body);

    // A date set by hand is no longer after the prerequisites, and the other way round.
    let r = t.mutate(&s, json!([{ "op": "task.update", "id": visit, "due": { "date": "2026-12-20" } }])).await;
    let v = task_in(&r.body["effects"], &visit);
    assert_eq!((v.get("due_after"), v.get("unlocked_on"), &v["due"]), (None, None, &json!({ "date": "2026-12-20" })));
    let r = post(made_on("2026-11-24", json!([{ "op": "task.update", "id": visit, "due_after": { "days": 0 } }])).1).await;
    assert_eq!(task_in(&r.body["effects"], &visit)["due"], json!({ "date": "2026-12-10" }), "{}", r.body);
    let r = t.mutate(&s, json!([{ "op": "task.update", "id": visit, "due_after": null }])).await;
    let v = task_in(&r.body["effects"], &visit);
    assert_eq!((v.get("due_after"), &v["due"]), (None, &Value::Null), "{}", r.body);
}

#[tokio::test]
async fn a_repeating_prerequisite_coming_round_again_leaves_the_date_it_gave() {
    let t = TestApp::new();
    let s = t.admin().await;
    let (water, log) = (id(), id());
    let daily = json!({ "date": "2026-11-20", "repeat": { "every": 1, "unit": "day", "from": "due" } });
    t.mutate(
        &s,
        json!([
            { "op": "task.create", "id": water, "title": "浇花", "due": daily },
            { "op": "task.create", "id": log, "title": "记下长势", "due_after": { "days": 1 } },
            { "op": "dep.add", "before": water, "after": log },
        ]),
    )
    .await;
    let post = |body: Value| t.call("POST", "/mutations", Some(&s), Some(body));
    // Done on the device's day, given with the task done.
    let (first, body) = made_on("2026-11-20", json!([{ "op": "task.update", "id": water, "done": true, "today": "2026-11-20" }]));
    let r = post(body).await;
    assert_eq!(task_in(&r.body["effects"], &log)["due"]["date"], "2026-11-21", "{}", r.body);
    let (_, body) = made_on("2026-11-23", json!([{ "op": "task.update", "id": log, "title": "记下花的长势" }]));
    let r = post(body).await;
    assert_eq!(task_in(&r.body["effects"], &log)["due"]["date"], "2026-11-21", "{}", r.body);
    // Deleted and brought back, it is as it was, the dependency with it.
    let (deleted, body) = made_on("2026-11-23", json!([{ "op": "task.delete", "id": log }]));
    post(body).await;
    let r = post(made_on("2026-11-23", json!([{ "op": "undo", "mutation": deleted }])).1).await;
    assert_eq!(task_in(&r.body["effects"], &log)["due"]["date"], "2026-11-21", "{}", r.body);
    // Done again the next day and that undone, it is that day's time again: the date stays. Undoing the
    // time that gave it takes it back.
    let (again, body) = made_on("2026-11-21", json!([{ "op": "task.update", "id": water, "done": true, "today": "2026-11-21" }]));
    post(body).await;
    post(made_on("2026-11-21", json!([{ "op": "undo", "mutation": again }])).1).await;
    let due = |snap: &Value| snap["tasks"].as_array().unwrap().iter().find(|x| x["id"] == log.as_str()).unwrap()["due"].clone();
    assert_eq!(due(&t.snapshot(&s).await)["date"], "2026-11-21");
    let r = post(made_on("2026-11-21", json!([{ "op": "undo", "mutation": first }])).1).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(due(&t.snapshot(&s).await), Value::Null);
}

#[tokio::test]
async fn a_dependency_an_undo_brings_back_to_a_prerequisite_reopened_since_takes_the_date_back() {
    let t = TestApp::new();
    let s = t.admin().await;
    let post = |body: Value| t.call("POST", "/mutations", Some(&s), Some(body));
    let (invoice, claim) = (id(), id());
    let ops = json!([
        { "op": "task.create", "id": invoice, "title": "收到发票", "done": true },
        { "op": "task.create", "id": claim, "title": "报销", "due_after": { "days": 7 } },
        { "op": "dep.add", "before": invoice, "after": claim },
    ]);
    let r = post(made_on("2026-11-20", ops).1).await;
    assert_eq!(task_in(&r.body["effects"], &claim)["due"]["date"], "2026-11-27", "{}", r.body);
    let (removed, body) = made_on("2026-11-21", json!([{ "op": "dep.remove", "before": invoice, "after": claim }]));
    post(body).await;
    t.mutate(&s, json!([{ "op": "task.update", "id": invoice, "done": false }])).await;
    let r = post(made_on("2026-11-22", json!([{ "op": "undo", "mutation": removed }])).1).await;
    assert_eq!(task_in(&r.body["effects"], &claim)["due"], Value::Null, "{}", r.body);
}

#[tokio::test]
async fn a_due_date_after_the_prerequisites_is_checked_as_given() {
    let t = TestApp::new();
    let s = t.admin().await;
    let both = json!([{ "op": "task.create", "id": id(), "title": "付款", "due": { "date": "2026-11-20" }, "due_after": { "days": 3 } }]);
    assert_eq!(t.mutate(&s, both).await.body["details"]["field"], "due_after");
    let late = json!([{ "op": "task.create", "id": id(), "title": "付款", "start": "2026-11-20", "start_time": "19:00", "due_after": { "days": 0, "time": "18:00" } }]);
    assert_eq!(t.mutate(&s, late).await.body["details"]["field"], "start_time");
    let far = json!([{ "op": "task.create", "id": id(), "title": "付款", "due_after": { "days": 366 } }]);
    assert_eq!(t.mutate(&s, far).await.status, StatusCode::BAD_REQUEST);
    let time = json!({ "id": id(), "time": "18:00", "ops": [{ "op": "task.create", "id": id(), "title": "付款" }] });
    assert_eq!(t.call("POST", "/mutations", Some(&s), Some(time)).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn settings_go_with_the_account_and_only_to_it() {
    let t = TestApp::new();
    let admin = t.admin().await;
    let bob = t.user(&admin, "bob").await;
    let since = t.snapshot(&bob).await["seq"].as_i64().unwrap();
    let set = |ops: Value| t.mutate(&admin, json!([ops]));

    let r = set(json!({ "op": "prefs.set", "accent": "blue", "time_grid": false, "keep_done": 5, "add_to": { "folder": null } })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        t.snapshot(&admin).await["prefs"],
        json!({ "accent": "blue", "time_grid": false, "keep_done": 5, "add_to": { "folder": null } })
    );
    // Null takes a setting back to the default; what is left out stays.
    set(json!({ "op": "prefs.set", "accent": "#336699", "keep_done": null })).await;
    assert_eq!(t.snapshot(&admin).await["prefs"], json!({ "accent": "#336699", "time_grid": false, "add_to": { "folder": null } }));

    for bad in [json!({ "accent": "red" }), json!({ "accent": "#33669" }), json!({ "keep_done": 21 }), json!({ "font": "big" })] {
        let mut op = json!({ "op": "prefs.set" });
        op.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert_eq!(set(op).await.status, StatusCode::BAD_REQUEST, "{bad}");
    }

    assert_eq!(t.snapshot(&bob).await["prefs"], json!({}));
    let changes = t.call("GET", &format!("/changes?since={since}"), Some(&bob), None).await.body;
    assert_eq!(changes["changes"], json!([]), "Bob does not see Alice's settings");
    // A device's own going in only fills in what is not set.
    set(json!({ "op": "prefs.set", "accent": "pink", "holidays": false, "if_unset": true })).await;
    assert_eq!(t.snapshot(&admin).await["prefs"]["accent"], "#336699");
    assert_eq!(t.snapshot(&admin).await["prefs"]["holidays"], false);
    // Settings are not changes to the todos: the history leaves them out, those that changed nothing too.
    set(json!({ "op": "prefs.set", "holidays": false })).await;
    assert_eq!(t.ok(&admin, "log", json!({})).await["data"], json!([]));
}
