//! How a task repeats, and the times it was done.

use super::{Date, TaskId};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use ts_rs::TS;

/// A repeating task stays open: done, its due date moves on, until the last time (docs/design/repeat.md#重复).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "RepeatFields", into = "RepeatFields")]
#[ts(export, as = "RepeatFields")]
pub struct Repeat {
    pub every: Every,
    pub period: Period,
    pub from: RepeatFrom,
    /// The last day it may fall on, if it ends; not before the due date.
    pub until: Option<Date>,
}

impl Repeat {
    /// Why it can't repeat from the due date `date`, if it can't. An undo, setting back the date or
    /// the repeat alone, may come to either.
    pub fn misfit(&self, date: &Date) -> Option<Misfit> {
        if self.until.as_ref().is_some_and(|until| until < date) {
            return Some(Misfit::Late);
        }
        (matches!(self.period, Period::LunarMonth(_)) && date.lunar().is_none()).then_some(Misfit::OffTable)
    }
}

/// Why a repeat can't go from a due date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Misfit {
    /// The date is after the day it repeats until.
    Late,
    /// It goes by the lunar calendar, and the date is outside its table.
    OffTable,
}

impl std::fmt::Display for Misfit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Misfit::Late => "重复到不能早于截止日期",
            Misfit::OffTable => "按农历重复的截止日期应在 1900-01-31 到 2101-01-28 之间",
        })
    }
}

/// What `every` counts: days; weeks, on some days of the week or the due date's; months, on some days
/// of the month or the due date's; lunar months, always on some days of them, the 1st to the 30th. A
/// year is 12 months. Or every week, Monday to Friday as the State Council sets the days around public
/// holidays: those off taken out, the weekends worked to make up for them put in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Day,
    Week(Option<Weekdays>),
    Month(Option<Monthdays>),
    LunarMonth(Monthdays),
    LegalWorkdays,
}

impl Period {
    pub fn unit(self) -> RepeatUnit {
        match self {
            Period::Day => RepeatUnit::Day,
            Period::Week(_) | Period::LegalWorkdays => RepeatUnit::Week,
            Period::Month(_) | Period::LunarMonth(_) => RepeatUnit::Month,
        }
    }
}

/// How `Repeat` looks in JSON.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "RepeatFields")]
pub struct RepeatFields {
    every: Every,
    unit: RepeatUnit,
    from: RepeatFrom,
    /// A weekly one's days of the week, Monday 1 to Sunday 7, or a monthly one's days of the month,
    /// 1–31, when not just the due date's; a lunar one's always, 1–30.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    days: Option<Vec<u8>>,
    /// Whether a monthly one goes by the lunar calendar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    lunar: bool,
    /// Whether one every week, Monday to Friday, goes by the days off and worked the State Council
    /// sets: 每个法定工作日.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    legal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    until: Option<Date>,
}

impl TryFrom<RepeatFields> for Repeat {
    type Error = &'static str;

    fn try_from(f: RepeatFields) -> Result<Self, Self::Error> {
        if f.legal {
            let workdays = f.days.map(Weekdays::try_from).transpose()? == Some(Weekdays::WORKDAYS);
            if f.unit != RepeatUnit::Week || f.every.get() != 1 || f.lunar || !workdays {
                return Err("每个法定工作日只能整组用，不能自选周几、每隔几周或按农历");
            }
            return Ok(Repeat { every: f.every, period: Period::LegalWorkdays, from: f.from, until: f.until });
        }
        let period = match (f.unit, f.days, f.lunar) {
            (RepeatUnit::Day, Some(_), _) => return Err("按天重复不能选周几或几号"),
            (RepeatUnit::Day | RepeatUnit::Week, _, true) => return Err("只有按月重复能按农历"),
            (RepeatUnit::Day, None, false) => Period::Day,
            (RepeatUnit::Week, days, false) => Period::Week(days.map(Weekdays::try_from).transpose()?),
            (RepeatUnit::Month, days, false) => Period::Month(days.map(Monthdays::try_from).transpose()?),
            (RepeatUnit::Month, Some(days), true) => Period::LunarMonth(Monthdays::lunar(days)?),
            (RepeatUnit::Month, None, true) => return Err("按农历重复要选几号（初一到三十）"),
        };
        Ok(Repeat { every: f.every, period, from: f.from, until: f.until })
    }
}

impl From<Repeat> for RepeatFields {
    fn from(r: Repeat) -> Self {
        let days = match r.period {
            Period::Day => None,
            Period::Week(days) => days.map(Vec::from),
            Period::Month(days) => days.map(Vec::from),
            Period::LunarMonth(days) => Some(Vec::from(days)),
            Period::LegalWorkdays => Some(Vec::from(Weekdays::WORKDAYS)),
        };
        let lunar = matches!(r.period, Period::LunarMonth(_));
        let legal = r.period == Period::LegalWorkdays;
        RepeatFields { every: r.every, unit: r.period.unit(), from: r.from, days, lunar, legal, until: r.until }
    }
}

/// How many times a repeating task has been done, and the last day it was, on the device that did
/// it: done, it moves on instead (docs/design/repeat.md#重复).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Completions {
    pub count: NonZeroU32,
    pub last: Date,
}

/// A time of a repeating task: the one due on `due` after it was done `completions` times. The date
/// alone does not tell: done early, from the day it is done, the next can be due on the same day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Occurrence {
    pub due: Date,
    pub completions: u32,
}

/// A time a repeating task was done (docs/design/repeat.md#重复): the time `due` on a day, `done` on a day, on
/// the device that did it. Each time it was done is kept: `again` is how often that time was done
/// before, 0 the first time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Completion {
    pub task_id: TaskId,
    pub due: Date,
    pub done: Date,
    // Missing in what was recorded while a time kept only its latest record: the only one.
    #[serde(default)]
    pub again: u32,
}

/// How many units apart the due dates of a repeating task are: 1–365.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "u32", into = "u32")]
#[ts(export, type = "number")]
pub struct Every(u32);

impl Every {
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for Every {
    type Error = &'static str;

    fn try_from(n: u32) -> Result<Self, Self::Error> {
        if (1..=365).contains(&n) { Ok(Every(n)) } else { Err("重复的间隔应为 1–365") }
    }
}

impl From<Every> for u32 {
    fn from(e: Every) -> u32 {
        e.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RepeatUnit {
    Day,
    Week,
    Month,
}

/// The days of the week a weekly repeat falls on, at least one: in JSON Monday is 1 and Sunday 7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weekdays(u8);

impl Weekdays {
    /// Monday to Friday.
    pub const WORKDAYS: Weekdays = Weekdays(0b001_1111);

    /// Whether it has the day `i` days after Monday.
    pub fn has(self, i: u32) -> bool {
        self.0 & 1 << i != 0
    }
}

impl TryFrom<Vec<u8>> for Weekdays {
    type Error = &'static str;

    fn try_from(days: Vec<u8>) -> Result<Self, Self::Error> {
        if days.is_empty() || days.iter().any(|d| !(1..=7).contains(d)) {
            return Err("重复的周几应为 1–7（周一到周日），至少一天");
        }
        Ok(Weekdays(days.iter().fold(0, |bits, d| bits | 1 << (d - 1))))
    }
}

impl From<Weekdays> for Vec<u8> {
    fn from(w: Weekdays) -> Vec<u8> {
        (1..=7).filter(|&d| w.has(u32::from(d) - 1)).collect()
    }
}

/// The days of the month a monthly repeat falls on, 1–31, at least one. A month without one of them
/// has its last day instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monthdays(u32);

impl Monthdays {
    /// Whether it has the day `d`, 1–31.
    pub fn has(self, d: u32) -> bool {
        self.0 & 1 << (d - 1) != 0
    }
}

impl Monthdays {
    /// The days of a lunar month: 1–30, 初一 to 三十.
    pub fn lunar(days: Vec<u8>) -> Result<Self, &'static str> {
        if days.iter().any(|&d| d > 30) {
            return Err("农历的几号应为 1–30（初一到三十）");
        }
        Monthdays::try_from(days)
    }
}

impl TryFrom<Vec<u8>> for Monthdays {
    type Error = &'static str;

    fn try_from(days: Vec<u8>) -> Result<Self, Self::Error> {
        if days.is_empty() || days.iter().any(|d| !(1..=31).contains(d)) {
            return Err("重复的几号应为 1–31，至少一天");
        }
        Ok(Monthdays(days.iter().fold(0, |bits, d| bits | 1 << (d - 1))))
    }
}

impl From<Monthdays> for Vec<u8> {
    fn from(m: Monthdays) -> Vec<u8> {
        (1..=31).filter(|&d| m.has(u32::from(d))).collect()
    }
}

/// What the next due date counts from: the one before, keeping to the schedule, or the day it is
/// done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RepeatFrom {
    Due,
    Done,
}

sql_text_enum!(RepeatUnit { Day = "day", Week = "week", Month = "month" });
sql_text_enum!(RepeatFrom { Due = "due", Done = "done" });

/// Stored as bits, Monday the lowest.
#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for Weekdays {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match u8::column_result(v)? {
            bits @ 1..=0b111_1111 => Ok(Weekdays(bits)),
            bits => Err(rusqlite::types::FromSqlError::OutOfRange(bits.into())),
        }
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for Weekdays {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.into())
    }
}

/// Stored as bits, the 1st the lowest.
#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for Monthdays {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match u32::column_result(v)? {
            bits @ 1..=0x7fff_ffff => Ok(Monthdays(bits)),
            bits => Err(rusqlite::types::FromSqlError::OutOfRange(bits.into())),
        }
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for Monthdays {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.into())
    }
}

#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for Every {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Every::try_from(u32::column_result(v)?).map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for Every {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn every_statutory_working_day_is_monday_to_friday_every_week() {
        let workdays = json!({ "every": 1, "unit": "week", "from": "due", "days": [1, 2, 3, 4, 5], "legal": true });
        let r = from_value::<Repeat>(workdays.clone()).unwrap();
        assert_eq!((r.period, to_value(&r).unwrap()), (Period::LegalWorkdays, workdays));
        for wrong in [
            json!({ "every": 2, "unit": "week", "from": "due", "days": [1, 2, 3, 4, 5], "legal": true }),
            json!({ "every": 1, "unit": "week", "from": "due", "days": [1, 3, 5], "legal": true }),
            json!({ "every": 1, "unit": "week", "from": "due", "legal": true }),
            json!({ "every": 1, "unit": "month", "from": "due", "days": [1, 2, 3, 4, 5], "legal": true }),
        ] {
            assert!(from_value::<Repeat>(wrong.clone()).is_err(), "{wrong}");
        }
        // Not written for the others: servers from before it refuse what they don't know.
        let plain = from_value::<Repeat>(json!({ "every": 1, "unit": "week", "from": "due", "days": [1, 2, 3, 4, 5] })).unwrap();
        assert_eq!(to_value(plain).unwrap().get("legal"), None);
    }
}
