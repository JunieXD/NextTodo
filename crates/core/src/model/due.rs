//! When a task is due, and its reminders: by the due date, the start, or a time of their own.

use super::{Date, Repeat, Time};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// When a task is due: a date, optionally with a time, its reminders, and how it repeats.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "DueFields", into = "DueFields")]
#[ts(export, as = "DueFields")]
pub struct Due {
    pub date: Date,
    pub at: Option<DueAt>,
    /// Reminders on the due date or days before it, at a time of day (docs/design.md#提醒). Sorted,
    /// none twice.
    pub remind_on: Vec<RemindOn>,
    pub repeat: Option<Repeat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DueAt {
    pub time: Time,
    /// Reminders so many minutes before the due time, 0 at it. Sorted, none twice.
    pub remind_before: Vec<u32>,
}

pub const MAX_REMIND_BEFORE: u32 = 30 * 24 * 60;
pub const MAX_REMIND_DAYS: u32 = 30;
/// Only against misuse: the app sets no limit.
pub const MAX_REMINDERS: usize = 100;

/// A reminder so many minutes before the due time, as JSON has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RemindBefore {
    pub before: u32,
}

/// A reminder on the due date, `days` 0, or so many days before it, at `time`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RemindOn {
    pub days: u32,
    pub time: Time,
}

/// A reminder of either kind, as JSON has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum Remind {
    Before(RemindBefore),
    On(RemindOn),
}

/// A reminder at a time of its own, ringing once (docs/design.md#提醒), by the clock of the device.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RemindAt {
    pub date: Date,
    pub time: Time,
}

/// Reminders by the start, `time` its time of day: checked, none twice, those on a day before those so
/// long before, each the first to come first. Not all by when they come, as that moves with the
/// start's time, a field of its own: moving it would change them too, and undoing it what was set
/// since (docs/design.md#改动记录和撤销).
pub fn start_reminders(time: Option<&Time>, reminders: Vec<Remind>) -> Result<Vec<Remind>, String> {
    let (at, mut on) = timing(time.cloned(), reminders, "开始")?;
    on.sort_by(|a, b| b.days.cmp(&a.days).then_with(|| a.time.cmp(&b.time)));
    let before = at.into_iter().flat_map(|at| at.remind_before.into_iter().rev()).map(|before| Remind::Before(RemindBefore { before }));
    Ok(on.into_iter().map(Remind::On).chain(before).collect())
}

/// The reminders a device gives a task as it gets a due date (docs/design.md#提醒): one due at a time
/// those so many minutes before, one due on a date alone those by the day. Sorted, none twice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "DefaultRemindersFields")]
#[ts(export)]
pub struct DefaultReminders {
    timed: Vec<RemindBefore>,
    dated: Vec<RemindOn>,
}

impl DefaultReminders {
    /// Those for a due date at a time, or else for one on a date alone.
    pub fn for_due(&self, timed: bool) -> Vec<Remind> {
        if timed { self.timed.iter().cloned().map(Remind::Before).collect() } else { self.dated.iter().cloned().map(Remind::On).collect() }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultRemindersFields {
    timed: Vec<RemindBefore>,
    dated: Vec<RemindOn>,
}

impl TryFrom<DefaultRemindersFields> for DefaultReminders {
    type Error = String;

    fn try_from(DefaultRemindersFields { mut timed, mut dated }: DefaultRemindersFields) -> Result<Self, String> {
        if timed.iter().any(|r| r.before > MAX_REMIND_BEFORE) || dated.iter().any(|r| r.days > MAX_REMIND_DAYS) {
            return Err("提醒最多提前 30 天".into());
        }
        timed.sort_by_key(|r| r.before);
        timed.dedup();
        dated.sort();
        dated.dedup();
        if timed.len().max(dated.len()) > MAX_REMINDERS {
            return Err(format!("提醒最多 {MAX_REMINDERS} 个"));
        }
        Ok(DefaultReminders { timed, dated })
    }
}

/// How `Due` looks in JSON.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "DueFields")]
pub struct DueFields {
    date: Date,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    time: Option<Time>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Remind>>")]
    reminders: Vec<Remind>,
    /// One reminder so many minutes before, as versions before several reminders wrote it: in older
    /// changes, drive commits and apps. Read, never written.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    remind_before: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    repeat: Option<Repeat>,
}

/// The time of day and the reminders of a due date, or of what else they go by, `of`: checked,
/// sorted, none twice.
fn timing(time: Option<Time>, reminders: Vec<Remind>, of: &str) -> Result<(Option<DueAt>, Vec<RemindOn>), String> {
    let (mut before, mut on) = (Vec::new(), Vec::new());
    for r in reminders {
        match r {
            Remind::Before(RemindBefore { before: m }) if m > MAX_REMIND_BEFORE => return Err("提醒最多提前 30 天".into()),
            Remind::On(RemindOn { days, .. }) if days > MAX_REMIND_DAYS => return Err("提醒最多提前 30 天".into()),
            Remind::Before(RemindBefore { before: m }) => before.push(m),
            Remind::On(o) => on.push(o),
        }
    }
    before.sort_unstable();
    before.dedup();
    on.sort();
    on.dedup();
    if before.len() + on.len() > MAX_REMINDERS {
        return Err(format!("提醒最多 {MAX_REMINDERS} 个"));
    }
    let at = match time {
        Some(time) => Some(DueAt { time, remind_before: before }),
        None if before.is_empty() => None,
        None => return Err(format!("提前多久的提醒需要具体的{of}时间")),
    };
    Ok((at, on))
}

/// The reminders of a due date at `at` with `on`, the first to come first.
fn in_order(at: Option<&DueAt>, on: &[RemindOn]) -> Vec<Remind> {
    let minutes = |t: &Time| {
        let (h, m) = t.as_str().split_once(':').expect("HH:MM");
        i64::from(h.parse::<u16>().expect("hours")) * 60 + i64::from(m.parse::<u16>().expect("minutes"))
    };
    // Minutes from the start of the due date.
    let mut all: Vec<(i64, Remind)> = at
        .iter()
        .flat_map(|at| at.remind_before.iter().map(|&m| (minutes(&at.time) - i64::from(m), Remind::Before(RemindBefore { before: m }))))
        .chain(on.iter().map(|o| (minutes(&o.time) - i64::from(o.days) * 1440, Remind::On(o.clone()))))
        .collect();
    all.sort_by_key(|(at, _)| *at);
    all.into_iter().map(|(_, r)| r).collect()
}

impl Due {
    pub fn new(date: Date, time: Option<Time>, reminders: Vec<Remind>, repeat: Option<Repeat>) -> Result<Due, String> {
        // Before the reminders, as editing says which is wrong (tools::write::edit_fields).
        if let Some(m) = repeat.as_ref().and_then(|r| r.misfit(&date)) {
            return Err(m.to_string());
        }
        let (at, remind_on) = timing(time, reminders, "截止")?;
        Ok(Due { date, at, remind_on, repeat })
    }

    /// The reminders, the first to come first.
    pub fn reminders(&self) -> Vec<Remind> {
        in_order(self.at.as_ref(), &self.remind_on)
    }
}

/// A due date some days after the task's prerequisites are all done (docs/design.md#前置完成后的截止):
/// the time of day and the reminders a due date has, without the date, which comes once they are.
/// Never repeating.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "DueAfterFields", into = "DueAfterFields")]
#[ts(export, as = "DueAfterFields")]
pub struct DueAfter {
    /// After the day they are all done, 0 for that day.
    pub days: u16,
    pub at: Option<DueAt>,
    pub remind_on: Vec<RemindOn>,
}

pub const MAX_DUE_AFTER_DAYS: u16 = 365;

/// How `DueAfter` looks in JSON.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "DueAfterFields")]
pub struct DueAfterFields {
    days: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    time: Option<Time>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Remind>>")]
    reminders: Vec<Remind>,
}

impl DueAfter {
    pub fn new(days: u16, time: Option<Time>, reminders: Vec<Remind>) -> Result<DueAfter, String> {
        if days > MAX_DUE_AFTER_DAYS {
            return Err(format!("前置完成后最多 {MAX_DUE_AFTER_DAYS} 天"));
        }
        let (at, remind_on) = timing(time, reminders, "截止")?;
        Ok(DueAfter { days, at, remind_on })
    }

    /// The due date it gives counting from `from`.
    pub fn due(&self, from: &Date) -> Due {
        Due { date: from.plus(u32::from(self.days)), at: self.at.clone(), remind_on: self.remind_on.clone(), repeat: None }
    }

    pub fn reminders(&self) -> Vec<Remind> {
        in_order(self.at.as_ref(), &self.remind_on)
    }
}

impl TryFrom<DueAfterFields> for DueAfter {
    type Error = String;

    fn try_from(f: DueAfterFields) -> Result<Self, String> {
        DueAfter::new(f.days, f.time, f.reminders)
    }
}

impl From<DueAfter> for DueAfterFields {
    fn from(w: DueAfter) -> Self {
        let reminders = w.reminders();
        DueAfterFields { days: w.days, time: w.at.map(|at| at.time), reminders }
    }
}

impl TryFrom<DueFields> for Due {
    type Error = String;

    fn try_from(f: DueFields) -> Result<Self, String> {
        let old = f.remind_before.map(|before| Remind::Before(RemindBefore { before }));
        Due::new(f.date, f.time, f.reminders.into_iter().chain(old).collect(), f.repeat)
    }
}

impl From<Due> for DueFields {
    fn from(d: Due) -> Self {
        let reminders = d.reminders();
        DueFields { date: d.date, time: d.at.map(|at| at.time), reminders, remind_before: None, repeat: d.repeat }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn due_round_trips_and_rejects_reminder_without_time() {
        let due = json!({ "date": "2026-11-02", "time": "18:00", "reminders": [{ "before": 60 }] });
        assert_eq!(to_value(from_value::<Due>(due.clone()).unwrap()).unwrap(), due);
        assert_eq!(to_value(from_value::<Due>(json!({ "date": "2026-11-20" })).unwrap()).unwrap(), json!({ "date": "2026-11-20" }));
        // The first to come first, each once; by the day on a date alone too.
        let several = json!({ "date": "2026-11-08", "time": "09:00", "reminders": [
            { "before": 0 }, { "days": 0, "time": "08:00" }, { "before": 1440 }, { "days": 1, "time": "20:00" }, { "before": 0 }
        ] });
        let sorted = [
            json!({ "before": 1440 }),
            json!({ "days": 1, "time": "20:00" }),
            json!({ "days": 0, "time": "08:00" }),
            json!({ "before": 0 }),
        ];
        assert_eq!(to_value(from_value::<Due>(several).unwrap()).unwrap()["reminders"], json!(sorted));
        let by_day = json!({ "date": "2026-11-08", "reminders": [{ "days": 1, "time": "20:00" }] });
        assert_eq!(to_value(from_value::<Due>(by_day.clone()).unwrap()).unwrap(), by_day);
        // As versions before several reminders wrote it.
        let old = from_value::<Due>(json!({ "date": "2026-11-02", "time": "18:00", "remind_before": 60 })).unwrap();
        assert_eq!(to_value(old).unwrap(), due);
        for bad in [
            json!({ "date": "2026-11-20", "reminders": [{ "before": 0 }] }),
            json!({ "date": "2026-11-20", "remind_before": 0 }),
            json!({ "date": "2026-11-20", "time": "08:00", "reminders": [{ "before": 50000 }] }),
            json!({ "date": "2026-11-20", "reminders": [{ "days": 31, "time": "08:00" }] }),
            json!({ "date": "2026-11-20", "reminders": [{ "days": 1 }] }),
            json!({ "date": "2026-11-20", "reminders": [{ "days": 1, "time": "8:00" }] }),
            json!({ "date": "2026-11-20", "reminders": (0..101).map(|d| json!({ "days": d % 31, "time": format!("{:02}:00", d / 31) })).collect::<Vec<_>>() }),
        ] {
            assert!(from_value::<Due>(bad.clone()).is_err(), "{bad}");
        }
        assert!(from_value::<Due>(json!({ "date": "2026-11-20", "when": "soon" })).is_err());
        let repeating = json!({ "date": "2026-11-20", "repeat": { "every": 2, "unit": "week", "from": "done" } });
        assert_eq!(to_value(from_value::<Due>(repeating.clone()).unwrap()).unwrap(), repeating);
        let days = json!({ "date": "2026-11-20", "repeat": { "every": 1, "unit": "week", "from": "due", "days": [5, 1, 3] } });
        assert_eq!(to_value(from_value::<Due>(days).unwrap()).unwrap()["repeat"]["days"], json!([1, 3, 5]));
        let monthdays = json!({ "date": "2026-11-20", "repeat": { "every": 1, "unit": "month", "from": "due", "days": [31, 1, 15] } });
        assert_eq!(to_value(from_value::<Due>(monthdays).unwrap()).unwrap()["repeat"]["days"], json!([1, 15, 31]));
        for bad in [
            json!({ "every": 0, "unit": "day", "from": "due" }),
            json!({ "every": 366, "unit": "day", "from": "due" }),
            json!({ "every": 1, "unit": "year", "from": "due" }),
            json!({ "every": 1, "unit": "week", "from": "due", "days": [] }),
            json!({ "every": 1, "unit": "week", "from": "due", "days": [0, 1] }),
            json!({ "every": 1, "unit": "week", "from": "due", "days": [8] }),
            json!({ "every": 1, "unit": "day", "from": "due", "days": [1] }),
            json!({ "every": 1, "unit": "month", "from": "due", "days": [0] }),
            json!({ "every": 1, "unit": "month", "from": "due", "days": [32] }),
        ] {
            assert!(from_value::<Due>(json!({ "date": "2026-11-20", "repeat": bad })).is_err(), "{bad}");
        }
    }
}
