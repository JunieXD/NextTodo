//! A task edited, done, opened again and given up: its text, start, due date and reminders, and what it unlocks.

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::TestApp;
use crate::{numbers, text};

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
    assert!(text(&shown).contains("截止：2026-11-21 09:00\n提醒：前一天 20:00、截止时、当天 09:00"), "{}", text(&shown));
    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-22" })).await;
    let due = t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"].clone();
    assert_eq!(due, json!({ "date": "2026-11-22", "reminders": [{ "days": 1, "time": "20:00" }, { "days": 0, "time": "09:00" }] }));
    t.ok(&s, "edit", json!({ "task": 1, "remind": [] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"]["reminders"], Value::Null);
    // As long before, in minutes, hours, days or weeks; a list also as one string, as models write them.
    t.ok(&s, "edit", json!({ "task": 1, "due": "2026-11-30 09:00", "remind": ["2h30m", "1d", "1w 08:00, 3d", "30m"] })).await;
    let due = t.ok(&s, "show", json!({ "task": 1 })).await["data"]["due"].clone();
    assert_eq!(
        due["reminders"],
        json!([{ "days": 7, "time": "08:00" }, { "before": 4320 }, { "before": 1440 }, { "before": 150 }, { "before": 30 }])
    );
    for bad in [
        json!("1x 20:00"),
        json!("25:00"),
        json!(["31d 09:00"]),
        json!("5w"),
        json!("1d 2h 09:00"),
        json!("24h 09:00"),
        json!("1 2m"),
        json!("2x"),
        json!("30m,,1d"),
    ] {
        assert_eq!(t.tool(&s, "edit", json!({ "task": 1, "remind": bad })).await.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(t.tool(&s, "edit", json!({ "task": 1 })).await.status, StatusCode::BAD_REQUEST, "nothing to change");
}

#[tokio::test]
async fn reminders_go_by_the_due_the_start_or_times_of_their_own() {
    let t = TestApp::new();
    let s = t.admin().await;
    assert_eq!(t.tool(&s, "add", json!({ "title": "领券", "remind": "start 0" })).await.status, StatusCode::BAD_REQUEST, "no start");
    // A time of its own needs no due.
    t.ok(&s, "add", json!({ "title": "领券", "remind": "2026-12-15 10:00" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(text(&shown).contains("提醒：2026-12-15 10:00"), "{}", text(&shown));
    assert_eq!(shown["data"]["remind_at"], json!([{ "date": "2026-12-15", "time": "10:00" }]));
    // All three kinds at once, in place of those it had, each the first to come first.
    t.ok(&s, "edit", json!({ "task": 1, "start": "2026-12-01 20:00", "due": "2026-12-10", "remind": ["1d 20:00", "start 30m", "start 09:00", "2026-12-05 12:00"] }))
        .await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(text(&shown).contains("提醒：前一天 20:00、开始当天 09:00、开始前 30 分钟、2026-12-05 12:00"), "{}", text(&shown));
    // The start's time moved past one by the day, they stay as they are, no change of theirs; undoing
    // that leaves those set since.
    let moved = t.ok(&s, "edit", json!({ "task": 1, "start": "2026-12-01 08:00" })).await;
    assert_eq!(moved["data"]["changes"], json!([{ "kind": "task_edited", "task": 1, "fields": ["start"] }]));
    assert_eq!(
        t.ok(&s, "show", json!({ "task": 1 })).await["data"]["start_reminders"],
        json!([{ "days": 0, "time": "09:00" }, { "before": 30 }])
    );
    t.ok(&s, "edit", json!({ "task": 1, "remind": ["1d 20:00", "start 07:00", "2026-12-05 12:00"] })).await;
    t.ok(&s, "undo", json!({ "batch": moved["data"]["batch"] })).await;
    let data = t.ok(&s, "show", json!({ "task": 1 })).await["data"].clone();
    assert_eq!((&data["start_time"], &data["start_reminders"]), (&json!("20:00"), &json!([{ "days": 0, "time": "07:00" }])));
    t.ok(&s, "edit", json!({ "task": 1, "remind": ["1d 20:00", "start 30m", "start 09:00", "2026-12-05 12:00"] })).await;
    // The start's time gone, those so long before it go; whole days on a date alone are at 09:00.
    let dated = t.ok(&s, "edit", json!({ "task": 1, "start": "2026-12-02" })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["start_reminders"], json!([{ "days": 0, "time": "09:00" }]));
    t.ok(&s, "edit", json!({ "task": 1, "remind": ["start 1d", "2026-12-05 12:00"] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 1 })).await["data"]["start_reminders"], json!([{ "days": 1, "time": "09:00" }]));
    // The start gone, all those by it; the others stay.
    t.ok(&s, "edit", json!({ "task": 1, "start": null })).await;
    let data = t.ok(&s, "show", json!({ "task": 1 })).await["data"].clone();
    assert_eq!((&data["start_reminders"], &data["remind_at"]), (&Value::Null, &json!([{ "date": "2026-12-05", "time": "12:00" }])));
    // Undone, they come back with the start they go by.
    t.ok(&s, "undo", json!({ "batch": dated["data"]["batch"] })).await;
    let data = t.ok(&s, "show", json!({ "task": 1 })).await["data"].clone();
    assert_eq!((&data["start"], &data["start_time"]), (&json!("2026-12-01"), &json!("20:00")));
    assert_eq!(data["start_reminders"], json!([{ "days": 0, "time": "09:00" }, { "before": 30 }]));
    // At most 100 of all kinds together.
    let many: Vec<String> = (0..100).map(|i| format!("2026-12-{:02} 10:{:02}", 20 + i / 60, i % 60)).collect();
    let too_many = [many.clone(), vec!["start 0".to_owned()]].concat();
    let r = t.tool(&s, "edit", json!({ "task": 1, "remind": too_many })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    t.ok(&s, "edit", json!({ "task": 1, "remind": many })).await;
    // Changed alone, those at times of their own are a change of its reminders.
    let fewer = t.ok(&s, "edit", json!({ "task": 1, "remind": many[1..] })).await;
    assert_eq!(fewer["data"]["changes"], json!([{ "kind": "task_edited", "task": 1, "fields": ["reminders"] }]));
}

#[tokio::test]
async fn a_task_given_up_ends_and_lets_go_of_those_after_it_or_takes_them_along() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "办签证" })).await;
    t.ok(&s, "add", json!({ "title": "订机票", "after": [1] })).await;
    t.ok(&s, "add", json!({ "title": "订酒店", "after": [2] })).await;
    t.ok(&s, "add", json!({ "title": "打卡", "due": "2026-11-16", "repeat": "1d" })).await;
    t.ok(&s, "add", json!({ "title": "写游记", "after": [3] })).await;
    t.ok(&s, "done", json!({ "tasks": [5] })).await;

    // Given up alone, it no longer holds up the next one, which says so.
    let dropped = t.ok(&s, "drop", json!({ "tasks": [1] })).await;
    assert!(text(&dropped).starts_with("放弃 #1 办签证") && text(&dropped).contains("解锁 #2 订机票"), "{}", text(&dropped));
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert_eq!(shown["data"]["status"], "dropped");
    assert!(text(&shown).contains("[已放弃]") && text(&shown).contains("放弃："), "{}", text(&shown));
    assert_eq!(t.tool(&s, "drop", json!({ "tasks": [1] })).await.status, StatusCode::CONFLICT);
    assert_eq!(dropped["data"]["changes"][0]["kind"], "task_dropped", "{}", dropped["data"]);
    t.ok(&s, "undo", json!({ "batch": dropped["data"]["batch"] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 2 })).await["data"]["status"], "waiting");

    // With those after it still open, theirs too, in one batch, which undoing takes back whole.
    let all = t.ok(&s, "drop", json!({ "tasks": [1], "with_after": true })).await;
    let statuses = |v: Value| {
        v["data"]["tasks"].as_array().unwrap().iter().map(|t| (t["number"].as_i64().unwrap(), t["status"].clone())).collect::<Vec<_>>()
    };
    let listed = statuses(t.ok(&s, "list", json!({ "folder": "/", "all": true })).await);
    for (n, want) in [(1, "dropped"), (2, "dropped"), (3, "dropped"), (5, "done")] {
        assert!(listed.contains(&(n, json!(want))), "#{n}: {listed:?}");
    }
    t.ok(&s, "undo", json!({ "batch": all["data"]["batch"] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 3 })).await["data"]["status"], "waiting");

    // A repeating one ends for good, without counting as done; reopened, it is open, not repeating.
    t.ok(&s, "drop", json!({ "tasks": [4] })).await;
    let shown = t.ok(&s, "show", json!({ "task": 4 })).await;
    assert_eq!((shown["data"]["status"].as_str(), &shown["data"]["due"]), (Some("dropped"), &json!({ "date": "2026-11-16" })));
    assert_eq!(shown["data"]["completions"], Value::Null);
    t.ok(&s, "reopen", json!({ "tasks": [4] })).await;
    assert_eq!(t.ok(&s, "show", json!({ "task": 4 })).await["data"]["status"], "ready");
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
async fn a_task_due_some_days_after_its_prerequisites_reads_so_until_they_are_done() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({ "title": "拿到发票", "folder": "/报销" })).await;
    let added = t.ok(&s, "add", json!({ "title": "报销", "folder": "/报销", "after": [1], "due_after": "30d 18:00", "remind": 60 })).await;
    assert!(text(&added).contains("报销"), "{}", text(&added));
    let shown = t.ok(&s, "show", json!({ "task": 2 })).await;
    assert!(text(&shown).contains("截止：前置完成后 30 天内 18:00\n提醒：提前 1 小时"), "{}", text(&shown));
    assert_eq!(shown["data"]["due_after"], json!({ "days": 30, "time": "18:00", "reminders": [{ "before": 60 }] }));

    let done = t.ok(&s, "done", json!({ "tasks": [1], "today": "2026-11-20" })).await;
    assert!(text(&done).contains("解锁 #2 报销"), "{}", text(&done));
    let line = text(&t.ok(&s, "list", json!({ "folder": "/报销" })).await).to_owned();
    assert!(line.contains("截止 2026-12-20 18:00（前置完成后 30 天内 18:00）"), "{line}");

    // Changing the reminders keeps it after the prerequisites; a date given makes it one.
    t.ok(&s, "edit", json!({ "task": 2, "remind": "1d 09:00" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 2 })).await["data"].clone();
    assert_eq!(
        (shown["due_after"]["reminders"].clone(), shown["due"]["date"].clone()),
        (json!([{ "days": 1, "time": "09:00" }]), json!("2026-12-20"))
    );
    let dated = t.ok(&s, "edit", json!({ "task": 2, "due": "2026-12-01" })).await["data"]["batch"].clone();
    let log = t.ok(&s, "log", json!({ "batch": dated })).await;
    assert!(text(&log).contains("截止：2026-12-20 18:00（前置完成后 30 天内 18:00） → 2026-12-01"), "{}", text(&log));
    let shown = t.ok(&s, "show", json!({ "task": 2 })).await["data"].clone();
    assert_eq!(
        (shown.get("due_after"), shown["due"].clone()),
        (None, json!({ "date": "2026-12-01", "reminders": [{ "days": 1, "time": "09:00" }] }))
    );
    let r = t.tool(&s, "edit", json!({ "task": 2, "due_after": "3d", "repeat": "1w" })).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("repeat")), "{}", r.body);
    let r = t.tool(&s, "add", json!({ "title": "付款", "due": "2026-12-01", "due_after": "3d" })).await;
    assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("due_after")), "{}", r.body);

    // Copied, it waits again for the copy of its prerequisite; one without any counts from the day.
    t.ok(&s, "edit", json!({ "task": 2, "due_after": "30d" })).await;
    t.ok(&s, "add", json!({ "title": "归档", "folder": "/报销", "due_after": "7d" })).await;
    t.ok(&s, "copy", json!({ "folder": "/报销", "today": "2026-11-25" })).await;
    let copy = t.ok(&s, "list", json!({ "folder": "/报销 副本" })).await["data"]["tasks"].clone();
    let by = |title: &str| copy.as_array().unwrap().iter().find(|x| x["title"] == title).unwrap().clone();
    assert_eq!((by("报销").get("due").cloned(), by("报销")["due_after"]["days"].clone()), (None, json!(30)), "{copy}");
    assert_eq!(by("归档")["due"]["date"], "2026-12-02", "{copy}");
}
