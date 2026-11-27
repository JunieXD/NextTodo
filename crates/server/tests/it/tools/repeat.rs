//! Repeating tasks: moving on, the times done and their records, the lunar calendar and working days.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::text;
use crate::common::{self, TestApp};

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
    // Always on some days, 初一 to 三十, the due date's unless said; and only monthly.
    t.ok(&s, "edit", json!({ "task": 1, "repeat": "1ly" })).await;
    assert!(text(&t.ok(&s, "show", json!({ "task": 1 })).await).contains("每年农历八月十五"));
    for repeat in ["1lm:31", "1lw:1"] {
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
async fn a_date_said_by_the_lunar_calendar_is_the_next_day_so() {
    let t = TestApp::new();
    let s = t.admin().await;
    // 2026-11-26 is 十月十八; the 腊月 after it has 29 days (fixtures/lunar.json has the calendar checked).
    let today = "2026-11-26";
    let plan = json!({ "tasks": [
        { "title": "给妈妈过生日", "due": "农历八月十五", "remind": "1d", "repeat": "1ly" },
        { "title": "上香", "start": "农历初一", "due": "农历 十五 09:00" },
        { "title": "吃年夜饭", "due": "农历腊月三十 18:00", "repeat": "1ly" },
        { "title": "看灯会", "due": "农历1月15日" },
        { "title": "今天", "due": "农历十月十八" },
    ] });
    t.ok(&s, "apply", json!({ "plan": plan, "today": today })).await;
    let show = |n: i64| t.ok(&s, "show", json!({ "task": n }));
    let birthday = show(1).await["data"].clone();
    assert_eq!(birthday["due"]["date"], "2027-09-15");
    assert_eq!(birthday["due"]["repeat"]["days"], json!([15]), "by the lunar calendar on the due date's day");
    assert_eq!(birthday["due"]["reminders"], json!([{ "days": 1, "time": "09:00" }]), "a day before a date alone is at 09:00");
    let incense = show(2).await["data"].clone();
    assert_eq!((incense["start"].as_str(), incense["due"].clone()), (Some("2026-12-09"), json!({ "date": "2026-12-23", "time": "09:00" })));
    let eve = show(3).await["data"]["due"].clone();
    assert_eq!(eve["date"], "2027-02-05", "a month with no 三十 has its last day");
    assert_eq!(eve["repeat"]["days"], json!([30]), "the day as said, 三十, not the 廿九 it falls on this year");
    assert_eq!(show(4).await["data"]["due"]["date"], "2027-02-20");
    assert_eq!(show(5).await["data"]["due"]["date"], today);
    t.ok(&s, "edit", json!({ "task": 4, "start": "农历正月初一", "today": today })).await;
    assert_eq!(show(4).await["data"]["start"], "2027-02-06");
    // Repeating on some days of every lunar month: the first of them to come from the month said (the one
    // it is, from now: today's only while its time is still to come) and from the start. 2026-12-09 is
    // 冬月初一, 2027-01-08 腊月初一.
    let repeating = json!({ "title": "上香", "due": "农历冬月初一 08:00", "repeat": "1lm:1,15", "today": "2026-12-09", "time": "10:00" });
    t.ok(&s, "add", repeating).await;
    assert_eq!(show(6).await["data"]["due"]["date"], "2026-12-23");
    // Today's only while its time is still to come.
    t.ok(&s, "add", json!({ "title": "打电话", "due": "农历十月十八 08:00", "today": today, "time": "10:00" })).await;
    assert_eq!(show(7).await["data"]["due"]["date"], "2027-11-15");
    let bill = json!({ "title": "交账单", "start": "农历腊月初一", "due": "农历腊月十五", "repeat": "1lm:15", "today": today });
    t.ok(&s, "add", bill).await;
    let bill = show(8).await["data"].clone();
    assert_eq!((bill["start"].as_str(), bill["due"]["date"].as_str()), (Some("2027-01-08"), Some("2027-01-22")));
    t.ok(&s, "add", json!({ "title": "上供", "due": "农历腊月初一", "repeat": "1lm:1,15", "today": today })).await;
    assert_eq!(show(9).await["data"]["due"]["date"], "2027-01-08", "from the month said");
    t.ok(&s, "add", json!({ "title": "去庙里", "start": "2027-01-10", "due": "2027-01-22", "today": today })).await;
    t.ok(&s, "edit", json!({ "task": 10, "due": "农历初一", "repeat": "1lm:1,15", "today": today })).await;
    assert_eq!(show(10).await["data"]["due"]["date"], "2027-01-22", "not before the start it keeps");
    // The month it is stays the one said when its last day's time has passed or the start is in the next.
    let last = json!({ "title": "对账", "due": "农历十月三十 08:00", "repeat": "1lm:1,30", "today": "2026-12-08", "time": "10:00" });
    t.ok(&s, "add", last).await;
    assert_eq!(show(11).await["data"]["due"]["date"], "2026-12-09");
    t.ok(&s, "add", json!({ "title": "放生", "start": "2026-12-12", "due": "农历十月十五", "repeat": "1lm:1,15", "today": today })).await;
    assert_eq!(show(12).await["data"]["due"]["date"], "2026-12-23");
    for bad in [
        "农历十三月初一",
        "农历八月三十一",
        "农历",
        "农历八月",
        "农历初一初二",
        "农历八月初十五",
        "农历八月十十",
        "农历十十十",
        "农历+8月15日",
        "农历一十月一十三",
    ] {
        let r = t.tool(&s, "add", json!({ "title": "x", "due": bad, "today": today })).await;
        assert_eq!((r.status, r.body["details"]["field"].as_str()), (StatusCode::BAD_REQUEST, Some("due")), "{bad}: {}", r.body);
    }
    let r = t.tool(&s, "add", json!({ "title": "x", "repeat": "1lm" })).await;
    assert_eq!((r.status, r.body["message"].as_str()), (StatusCode::BAD_REQUEST, Some("设置重复需要截止时间")), "{}", r.body);
    // Not some hours before a day: there is no time to count from.
    let r = t.tool(&s, "add", json!({ "title": "x", "due": "2026-12-01", "remind": "2h", "today": today })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
}

#[tokio::test]
async fn every_statutory_working_day_skips_the_days_off_and_takes_the_weekends_worked() {
    let t = TestApp::new();
    let s = t.admin().await;
    // 春节 2027 is off from 2027-02-05 to 02-13, and Sunday 02-14 is worked (crates/core/src/holidays.json).
    t.ok(&s, "add", json!({ "title": "写日报", "due": "2027-02-04", "repeat": "workdays" })).await;
    let shown = t.ok(&s, "show", json!({ "task": 1 })).await;
    assert!(text(&shown).contains("截止：2027-02-04，每个法定工作日"), "{}", text(&shown));
    let repeat = json!({ "every": 1, "unit": "week", "from": "due", "days": [1, 2, 3, 4, 5], "legal": true });
    assert_eq!(shown["data"]["due"], json!({ "date": "2027-02-04", "repeat": repeat }));
    let done = t.ok(&s, "done", json!({ "tasks": [1], "today": "2027-02-04" })).await;
    assert!(text(&done).starts_with("顺延 #1 写日报 → 2027-02-14"), "{}", text(&done));
    let skipped = t.ok(&s, "skip", json!({ "tasks": [1], "today": "2027-02-14" })).await;
    assert!(text(&skipped).starts_with("顺延 #1 写日报 → 2027-02-15"), "{}", text(&skipped));
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
    assert!(text(&shown).contains("截止：2026-11-16 10:00，每周\n提醒：提前 15 分钟"), "{}", text(&shown));
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
async fn completion_tools_backdate_by_the_local_day_and_clock() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({"title":"组会"})).await;
    t.ok(&s, "add", json!({"title":"打卡","due":"2025-10-30","repeat":"1d","repeat_from":"done"})).await;
    let tool = t.ok(&s, "done", json!({"tasks":[1,2],"today":"2025-11-02","time":"02:26","at":"2025-11-01 20:30"})).await;
    assert!(tool["data"]["batch"].is_number());
    let snap = t.snapshot(&s).await;
    let by = |n: u32| snap["tasks"].as_array().unwrap().iter().find(|v| v["number"] == n).unwrap();
    assert_eq!(by(1)["done_at"], 1_761_978_600_000_i64);
    assert_eq!(by(2)["due"]["date"], "2025-11-02");
    assert_eq!(snap["completions"][0]["done"], "2025-11-01");
    t.ok(&s, "edit", json!({"task":1,"today":"2025-11-02","time":"02:26","done_at":"2025-10-31"})).await;
    t.ok(&s, "edit", json!({"task":2,"today":"2025-11-02","time":"02:26","done_at":"2025-10-30"})).await;
    let snap = t.snapshot(&s).await;
    let by = |n: u32| snap["tasks"].as_array().unwrap().iter().find(|v| v["number"] == n).unwrap();
    assert_eq!(by(1)["done_at"], 1_761_892_200_000_i64);
    assert_eq!(by(2)["due"]["date"], "2025-10-31");
    assert_eq!(snap["completions"][0]["done"], "2025-10-30");
    t.ok(&s, "edit", json!({"task":2,"today":"2025-11-02","done_at":"2025-10-28"})).await;
    t.ok(&s, "done", json!({"tasks":[2],"at":"2025-10-31"})).await;
    t.ok(&s, "edit", json!({"task":2,"today":"2025-11-02","done_at":"2025-10-29"})).await;
    let snap = t.snapshot(&s).await;
    let records = snap["completions"].as_array().unwrap();
    assert!(records.iter().any(|r| r["due"] == "2025-10-29" && r["done"] == "2025-10-29"));
    assert!(records.iter().any(|r| r["due"] == "2025-10-30" && r["done"] == "2025-10-28"));
    let r = t.tool(&s, "done", json!({"tasks":[1],"today":"2025-11-02","time":"02:26","at":"2025-11-02 02:27"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["details"]["field"], "at");
}

#[tokio::test]
async fn a_repeat_can_be_backdated_to_the_epoch_but_not_before() {
    let t = TestApp::new();
    let s = t.admin().await;
    t.ok(&s, "add", json!({"title":"打卡","due":"1970-01-01","repeat":"1d","repeat_from":"done"})).await;
    let r = t.tool(&s, "done", json!({"tasks":[1],"at":"1969-12-31"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["details"]["field"], "at");
    assert_eq!(r.body["message"], "完成日期不能早于 1970-01-01");
    assert_eq!(t.snapshot(&s).await["completions"], json!([]));
    t.ok(&s, "done", json!({"tasks":[1],"at":"1970-01-01"})).await;
    let snap = t.snapshot(&s).await;
    assert_eq!(snap["completions"][0]["done"], "1970-01-01");
    assert_eq!(snap["tasks"][0]["due"]["date"], "1970-01-02");
}
