//! Deadline reminders while the app runs. The web app hands over the ones to come whenever they
//! change (`apps/web/src/macos.ts`); a thread waits for the earliest and shows it.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

/// The longest wait between looks at the clock. The wait's own clock stops while the Mac sleeps, so
/// a reminder due meanwhile would otherwise come as late as the sleep was long.
const LOOK_AGAIN: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
pub struct Reminder {
    title: String,
    body: String,
    /// When to show it, in Unix milliseconds.
    at: i64,
}

#[derive(Default)]
pub struct Reminders {
    list: Mutex<Vec<Reminder>>,
    changed: Condvar,
}

#[tauri::command]
pub fn set_reminders(state: State<Reminders>, reminders: Vec<Reminder>) {
    *state.list.lock().expect("never poisoned") = reminders;
    state.changed.notify_one();
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("after 1970").as_millis() as i64
}

/// Shows each reminder when its time comes.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<Reminders>();
        let mut list = state.list.lock().expect("never poisoned");
        loop {
            let now = now_ms();
            let (due, rest): (Vec<_>, Vec<_>) = list.drain(..).partition(|r| r.at <= now);
            *list = rest;
            for r in due {
                if let Err(e) = app.notification().builder().title(&r.title).body(&r.body).show() {
                    eprintln!("提醒没能弹出：{e}");
                }
            }
            let wait = list.iter().map(|r| r.at - now).min().map_or(LOOK_AGAIN, |ms| LOOK_AGAIN.min(Duration::from_millis(ms as u64)));
            list = state.changed.wait_timeout(list, wait).expect("never poisoned").0;
        }
    });
}
