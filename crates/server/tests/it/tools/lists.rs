//! What is next, the agenda, lists and search, and how far they go.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{numbers, text};
use crate::common::TestApp;

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
         2026-11-21（明天）：\n  #3 [可做] 买菜 · 截止 2026-11-21\n2026-11-22（小雪）\n2026-11-27：\n  #2 开例会 · /工作 · 截止 2026-11-20 10:00，每周 · 以后的一次"
    );
    let data = &agenda["data"];
    assert_eq!(numbers(&data["overdue"]), [1]);
    let dates: Vec<&str> = data["days"].as_array().unwrap().iter().map(|d| d["date"].as_str().unwrap()).collect();
    assert_eq!(dates, ["2026-11-20", "2026-11-21", "2026-11-27"]);
    let later = &data["days"][2]["tasks"][0];
    assert_eq!((later["later"].as_bool(), later["due"]["date"].as_str()), (Some(true), Some("2026-11-20")), "due as it is now");
    assert!(data["days"][0]["tasks"][0].get("later").is_none());
    assert_eq!(data["marks"], json!([{ "date": "2026-11-22", "term": "小雪" }]));

    let work = t.ok(&s, "agenda", json!({ "today": "2026-11-20", "folders": ["/工作"] })).await;
    assert_eq!((numbers(&work["data"]["overdue"]), numbers(&work["data"]["days"][0]["tasks"])), (vec![1], vec![2]), "a week, in /工作");
    assert_eq!(work["data"]["days"].as_array().unwrap().len(), 1);
    let none = t.ok(&s, "agenda", json!({ "today": "2026-11-01", "days": 1 })).await;
    assert_eq!(text(&none), "今天起 1 天没有截止的事项，也没有逾期的。");
    assert_eq!(none["data"], json!({ "overdue": [], "days": [], "marks": [] }));
    // The most it takes: the “月历” widget fetches up to 73.
    t.ok(&s, "agenda", json!({ "days": 90 })).await;
    for days in [0, 91] {
        assert_eq!(t.tool(&s, "agenda", json!({ "days": days })).await.status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn agenda_says_the_festivals_and_the_days_off() {
    let t = TestApp::new();
    let s = t.admin().await;
    let none = t.ok(&s, "agenda", json!({ "today": "2027-02-06", "days": 1 })).await;
    assert_eq!(text(&none), "今天起 1 天没有截止的事项，也没有逾期的。\n2027-02-06（今天，春节，放假）");
    t.ok(&s, "add", json!({ "title": "贴春联", "due": "2027-02-05" })).await;
    let agenda = t.ok(&s, "agenda", json!({ "today": "2027-02-04", "days": 4 })).await;
    assert_eq!(
        text(&agenda),
        "2027-02-04（今天，立春）\n2027-02-05（明天，除夕，放假）：\n  #1 [可做] 贴春联 · 截止 2027-02-05\n2027-02-06（春节，放假）\n2027-02-07（放假）"
    );
    assert_eq!(
        agenda["data"]["marks"],
        json!([
            { "date": "2027-02-04", "term": "立春" },
            { "date": "2027-02-05", "festival": "除夕", "holiday": "off" },
            { "date": "2027-02-06", "festival": "春节", "holiday": "off" },
            { "date": "2027-02-07", "holiday": "off" },
        ])
    );
    let work = t.ok(&s, "agenda", json!({ "today": "2027-02-14", "days": 1 })).await;
    assert_eq!(work["data"]["marks"], json!([{ "date": "2027-02-14", "holiday": "work" }]));
    t.ok(&s, "add", json!({ "title": "写总结", "folder": "/工作", "due": "2027-03-01" })).await;
    let work = t.ok(&s, "agenda", json!({ "today": "2027-02-04", "days": 2, "folders": ["/工作"] })).await;
    assert_eq!(
        text(&work),
        "这些文件夹里今天起 2 天没有截止的事项，也没有逾期的。\n2027-02-04（今天，立春）\n2027-02-05（明天，除夕，放假）"
    );
    assert_eq!((&work["data"]["days"], work["data"]["marks"].as_array().unwrap().len()), (&json!([]), 2), "whatever the folders");
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
