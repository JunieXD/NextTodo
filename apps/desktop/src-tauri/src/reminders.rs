//! Deadline reminders while the app runs. The web app hands over the ones to come whenever they
//! change (`apps/web/src/desktop.ts`); a thread waits for the earliest and shows it.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

/// The longest wait between looks at the clock. The wait's own clock stops while the computer sleeps, so
/// a reminder due meanwhile would otherwise come as late as the sleep was long.
const LOOK_AGAIN: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
pub struct Reminder {
    title: String,
    body: String,
    /// When to show it, in Unix milliseconds.
    at: i64,
    /// The task, which a click on the reminder opens.
    task: String,
}

#[derive(Default)]
pub struct Reminders {
    queue: Mutex<Queue>,
    changed: Condvar,
}

#[derive(Default)]
struct Queue {
    list: Vec<Reminder>,
    /// When the thread last looked at the clock and showed what was due by then.
    looked: i64,
}

/// The kind of notification a reminder is: one with an 打开 button. Only with a button does macOS
/// wait for a click on the notification and report it.
pub const KIND: &str = "reminder";
pub const OPEN: &str = "open";

/// Takes over the reminders to come, and those due since the last handover that are still wanted.
#[tauri::command]
pub fn set_reminders(state: State<Reminders>, reminders: Vec<Reminder>) {
    let mut queue = state.queue.lock().expect("never poisoned");
    queue.list = kept(&queue.list, reminders, queue.looked, now_ms());
    state.changed.notify_one();
}

/// Of `reminders`, those to come, and those due since the thread last `looked` that `list` had already,
/// as when the computer has just woken: they are still shown, once. One just set in the app for a time
/// already past is not, as on Android. Those due before it looked were shown then, if they were to be;
/// unless the clock has gone back since.
fn kept(list: &[Reminder], reminders: Vec<Reminder>, looked: i64, now: i64) -> Vec<Reminder> {
    let shown = looked.min(now);
    let scheduled = |r: &Reminder| list.iter().any(|s| s.task == r.task && s.at == r.at);
    reminders.into_iter().filter(|r| r.at > now || (r.at > shown && scheduled(r))).collect()
}

/// Takes back every reminder, those due but not shown yet too: signed out.
#[tauri::command]
pub fn clear_reminders(state: State<Reminders>) {
    state.queue.lock().expect("never poisoned").list.clear();
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("after 1970").as_millis() as i64
}

/// Shows each reminder when its time comes.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<Reminders>();
        let mut queue = state.queue.lock().expect("never poisoned");
        loop {
            let now = now_ms();
            let (due, rest): (Vec<_>, Vec<_>) = queue.list.drain(..).partition(|r| r.at <= now);
            queue.list = rest;
            queue.looked = now;
            for r in due {
                if let Err(e) =
                    app.notification().builder().title(&r.title).body(&r.body).action_type_id(KIND).extra("task", &r.task).show()
                {
                    eprintln!("提醒弹出失败：{e}");
                }
            }
            let wait =
                queue.list.iter().map(|r| r.at - now).min().map_or(LOOK_AGAIN, |ms| LOOK_AGAIN.min(Duration::from_millis(ms as u64)));
            queue = state.changed.wait_timeout(queue, wait).expect("never poisoned").0;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reminder(task: &str, at: i64) -> Reminder {
        Reminder { title: task.into(), body: String::new(), at, task: task.into() }
    }

    fn tasks(list: Vec<Reminder>) -> Vec<String> {
        list.into_iter().map(|r| r.task).collect()
    }

    #[test]
    fn keeps_what_is_to_come_and_what_was_scheduled_and_missed() {
        let list = [reminder("missed", 150), reminder("moved", 150)];
        let handed = vec![
            reminder("shown", 50),
            reminder("missed", 150),
            reminder("moved", 160),
            reminder("set-past", 150),
            reminder("to-come", 300),
        ];
        assert_eq!(tasks(kept(&list, handed, 100, 200)), ["missed", "to-come"]);
    }

    #[test]
    fn clock_gone_back_keeps_only_what_is_to_come() {
        let list = [reminder("a", 150)];
        assert_eq!(tasks(kept(&list, vec![reminder("a", 150), reminder("b", 250)], 300, 200)), ["b"]);
    }
}
