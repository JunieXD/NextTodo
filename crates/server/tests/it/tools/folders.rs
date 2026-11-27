//! Folders named by path and managed, copied with all in them, and their colours and icons.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::text;
use crate::common::{self, TestApp};

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
    // A colour of one's own, in either case, is kept in lower case.
    t.ok(&s, "style", json!({ "folder": "/学习", "color": "#3478F6" })).await;
    assert_eq!(looks(t.ok(&s, "tree", json!({})).await["data"].clone())[1].1, json!("#3478f6"));
    let mut bad = vec![json!({ "folder": "/学习" }), json!({ "folder": "/", "icon": "book" })];
    bad.extend(["black", "3478f6", "#3478f", "#3478f60", "#34g8f6", "#+478f6"].map(|c| json!({ "folder": "/学习", "color": c })));
    for bad in bad {
        assert_eq!(t.tool(&s, "style", bad.clone()).await.status, StatusCode::BAD_REQUEST, "{bad}");
    }
}
