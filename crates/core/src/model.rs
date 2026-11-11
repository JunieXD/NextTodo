//! Entities and the validated values they are made of.

use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use uuid::Uuid;

pub use crate::sort_key::SortKey;

/// Ids are UUIDv7, so they sort by creation time.
macro_rules! id_type {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[serde(try_from = "Uuid", into = "Uuid")]
        #[ts(export, type = "string")]
        pub struct $name(pub Uuid);

        impl TryFrom<Uuid> for $name {
            type Error = &'static str;

            fn try_from(id: Uuid) -> Result<Self, Self::Error> {
                if id.get_version_num() == 7 && id.get_variant() == uuid::Variant::RFC4122 { Ok(Self(id)) } else { Err("ID 必须是 UUIDv7") }
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.0.to_string().into())
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                Uuid::parse_str(v.as_str()?).map(Self).map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    )*};
}

id_type!(FolderId, TaskId, MutationId, AiJobId);

/// Validated text stored as is. Values read back from the database were validated when written.
macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $check:expr) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[serde(try_from = "String", into = "String")]
        #[ts(export, type = "string")]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(s: String) -> Result<Self, String> {
                let check: fn(String) -> Result<String, String> = $check;
                check(s).map(Self)
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.0
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.0.as_str().into())
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                Ok(Self(v.as_str()?.to_owned()))
            }
        }
    };
}

/// An enum stored as the text it has in JSON, such as `waiting` or `day`.
macro_rules! sql_text_enum {
    ($name:ident { $($variant:ident = $text:literal),* }) => {
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),*
                }
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        #[cfg(feature = "sql")]
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                match v.as_str()? {
                    $($text => Ok($name::$variant),)*
                    _ => Err(rusqlite::types::FromSqlError::InvalidType),
                }
            }
        }
    };
}

/// Trims and checks a single-line value.
pub fn line(s: String, what: &str, max: usize) -> Result<String, String> {
    let s = s.trim().to_owned();
    if s.is_empty() {
        return Err(format!("{what}不能为空"));
    }
    if s.contains(['\n', '\r']) {
        return Err(format!("{what}不能换行"));
    }
    if s.chars().any(char::is_control) {
        return Err(format!("{what}不能包含控制字符"));
    }
    if s.chars().count() > max {
        return Err(format!("{what}不能超过 {max} 个字"));
    }
    Ok(s)
}

text_type!(
    /// Folder name. Without `/`, so that paths name folders.
    Name,
    |s| {
        let s = line(s, "名称", 100)?;
        if s.contains('/') { Err("文件夹名称不能包含 /".into()) } else { Ok(s) }
    }
);
text_type!(
    /// Task title.
    Title,
    |s| line(s, "标题", 200)
);
text_type!(
    /// Task description in Markdown; may be empty.
    Description,
    |s| if s.chars().count() > 20_000 {
        Err("描述不能超过 20000 个字".into())
    } else if s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        Err("描述不能包含控制字符".into())
    } else {
        Ok(s)
    }
);
text_type!(
    /// What was written for AI to organize into tasks.
    JobText,
    |s| if s.trim().is_empty() {
        Err("内容不能为空".into())
    } else if s.chars().count() > 10_000 {
        Err("内容不能超过 10000 个字".into())
    } else if s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        Err("内容不能包含控制字符".into())
    } else {
        Ok(s)
    }
);
text_type!(
    /// The user's local time as written by their device, such as `2026-11-04 16:30 星期三`: what
    /// "tomorrow" means in a text submitted for AI to organize.
    LocalTime,
    |s| line(s, "本地时间", 40)
);
text_type!(
    /// A calendar date `YYYY-MM-DD` in the user's local time.
    Date,
    |s| {
        let digits = |r: std::ops::Range<usize>| {
            s.get(r).filter(|p| p.bytes().all(|c| c.is_ascii_digit())).and_then(|p| p.parse::<u32>().ok())
        };
        let valid = match (s.len() == 10 && s.get(4..5) == Some("-") && s.get(7..8) == Some("-"), digits(0..4), digits(5..7), digits(8..10)) {
            (true, Some(y), Some(m), Some(d)) => (1..=12).contains(&m) && (1..=days_in_month(y, m)).contains(&d),
            _ => false,
        };
        if valid { Ok(s) } else { Err(format!("日期“{s}”格式错误，应为 YYYY-MM-DD")) }
    }
);
text_type!(
    /// A time of day `HH:MM` in the user's local time.
    Time,
    |s| {
        let digits = |r: std::ops::Range<usize>| {
            s.get(r).filter(|p| p.bytes().all(|c| c.is_ascii_digit())).and_then(|p| p.parse::<u32>().ok())
        };
        match (s.len() == 5 && s.get(2..3) == Some(":"), digits(0..2), digits(3..5)) {
            (true, Some(h), Some(m)) if h < 24 && m < 60 => Ok(s),
            _ => Err(format!("时间“{s}”格式错误，应为 HH:MM")),
        }
    }
);

/// The days in month `m` (1–12) of year `y`.
pub fn days_in_month(y: u32, m: u32) -> u32 {
    match m {
        4 | 6 | 9 | 11 => 30,
        2 if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) => 29,
        2 => 28,
        _ => 31,
    }
}

/// When a task is due: a date, optionally with a time and a reminder, and how it repeats.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "DueFields", into = "DueFields")]
#[ts(export, as = "DueFields")]
pub struct Due {
    pub date: Date,
    pub at: Option<DueAt>,
    pub repeat: Option<Repeat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DueAt {
    pub time: Time,
    /// Minutes before the due time; 0 means at the due time.
    pub remind_before: Option<u32>,
}

pub const MAX_REMIND_BEFORE: u32 = 30 * 24 * 60;

/// A repeating task stays open: done, its due date moves on (docs/design.md#重复).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "RepeatFields", into = "RepeatFields")]
#[ts(export, as = "RepeatFields")]
pub struct Repeat {
    pub every: Every,
    pub period: Period,
    pub from: RepeatFrom,
}

/// What `every` counts: days; weeks, on some days of the week or the due date's; months, on some days
/// of the month or the due date's. A year is 12 months.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Day,
    Week(Option<Weekdays>),
    Month(Option<Monthdays>),
}

impl Period {
    pub fn unit(self) -> RepeatUnit {
        match self {
            Period::Day => RepeatUnit::Day,
            Period::Week(_) => RepeatUnit::Week,
            Period::Month(_) => RepeatUnit::Month,
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
    /// 1–31, when not just the due date's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    days: Option<Vec<u8>>,
}

impl TryFrom<RepeatFields> for Repeat {
    type Error = &'static str;

    fn try_from(f: RepeatFields) -> Result<Self, Self::Error> {
        let period = match (f.unit, f.days) {
            (RepeatUnit::Day, None) => Period::Day,
            (RepeatUnit::Day, Some(_)) => return Err("按天重复不能选周几或几号"),
            (RepeatUnit::Week, days) => Period::Week(days.map(Weekdays::try_from).transpose()?),
            (RepeatUnit::Month, days) => Period::Month(days.map(Monthdays::try_from).transpose()?),
        };
        Ok(Repeat { every: f.every, period, from: f.from })
    }
}

impl From<Repeat> for RepeatFields {
    fn from(r: Repeat) -> Self {
        let days = match r.period {
            Period::Day => None,
            Period::Week(days) => days.map(Vec::from),
            Period::Month(days) => days.map(Vec::from),
        };
        RepeatFields { every: r.every, unit: r.period.unit(), from: r.from, days }
    }
}

/// How many times a repeating task has been done, and the last day it was, on the device that did
/// it: done, it moves on instead (docs/design.md#重复).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Completions {
    pub count: NonZeroU32,
    pub last: Date,
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

/// How `Due` looks in JSON.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "DueFields")]
pub struct DueFields {
    date: Date,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    time: Option<Time>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    remind_before: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    repeat: Option<Repeat>,
}

impl Due {
    pub fn new(date: Date, time: Option<Time>, remind_before: Option<u32>, repeat: Option<Repeat>) -> Result<Due, String> {
        if remind_before.is_some_and(|m| m > MAX_REMIND_BEFORE) {
            return Err("提醒最多提前 30 天".into());
        }
        let at = match (time, remind_before) {
            (Some(time), remind_before) => Some(DueAt { time, remind_before }),
            (None, Some(_)) => return Err("设置提醒需要具体的截止时间".into()),
            (None, None) => None,
        };
        Ok(Due { date, at, repeat })
    }
}

impl TryFrom<DueFields> for Due {
    type Error = String;

    fn try_from(f: DueFields) -> Result<Self, String> {
        Due::new(f.date, f.time, f.remind_before, f.repeat)
    }
}

impl From<Due> for DueFields {
    fn from(d: Due) -> Self {
        let (time, remind_before) = d.at.map_or((None, None), |at| (Some(at.time), at.remind_before));
        DueFields { date: d.date, time, remind_before, repeat: d.repeat }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Folder {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: Name,
    pub sort_key: SortKey,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Task {
    pub id: TaskId,
    pub number: i64,
    pub folder_id: Option<FolderId>,
    pub title: Title,
    pub description: Description,
    pub done_at: Option<i64>,
    pub due: Option<Due>,
    /// None until a repeating task is first done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completions: Option<Completions>,
    pub sort_key: SortKey,
    pub created_at: i64,
    pub updated_at: i64,
    /// Name of the API token that created the task; `None` when created by the user on a device.
    pub created_via: Option<String>,
    pub updated_via: Option<String>,
}

/// `before` must be done before `after` can start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Dependency {
    pub before: TaskId,
    pub after: TaskId,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobState {
    Waiting,
    Running,
    Failed,
    /// A tidying's suggestions are there to be taken.
    Ready,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobKind {
    /// Organizing the text into tasks.
    #[default]
    Organize,
    /// Suggesting changes to the folder's open tasks, the text saying what to look for.
    Tidy,
}

sql_text_enum!(JobState { Waiting = "waiting", Running = "running", Failed = "failed", Ready = "ready" });
sql_text_enum!(JobKind { Organize = "organize", Tidy = "tidy" });

/// What AI suggests doing in the folder it tidies, for the user to take or leave part by part. Each
/// kind is told by a field only it has (`task`, `delete`, `create`, `order`), with no tag: changes
/// to a task came first, and are kept as they were then.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum Suggestion {
    Edit(TaskEdit),
    Delete(TaskDeletion),
    Create(NewTask),
    Order(Reorder),
}

/// Changes to a task, the fields given set and the dependencies given added or removed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskEdit {
    pub task: TaskId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<Title>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<Description>,
    /// `null` clears the due date.
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due: Option<Option<Due>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub done: Option<bool>,
    /// A folder inside the one tidied, to move it into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder: Option<FolderId>,
    /// Tasks to put before it.
    pub after: Vec<TaskId>,
    /// Tasks before it now, to be so no more; none in suggestions made before there were these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlink: Vec<TaskId>,
    /// What changes and why, in a sentence.
    pub reason: String,
    /// The task as the model saw it: a field changed since is the user's newer word, not to be
    /// overwritten unasked.
    pub was: Was,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskDeletion {
    pub delete: TaskId,
    pub reason: String,
    pub was: Was,
}

/// A task to add to the folder tidied, under the id it is to have.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewTask {
    pub create: TaskId,
    pub title: Title,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<Description>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due: Option<Due>,
    /// Tasks to put before it, new ones among them.
    pub after: Vec<TaskId>,
    /// Tasks already there to put after it.
    pub before: Vec<TaskId>,
    pub reason: String,
}

/// Some of the folder's open tasks in a new order among themselves, in the places they hold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Reorder {
    pub order: Vec<TaskId>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Was {
    pub title: Title,
    pub description: Description,
    pub due: Option<Due>,
}

/// Tells a missing field (`None`) from an explicit `null` (`Some(None)`).
pub(crate) fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

/// Text submitted for AI to organize into tasks in a folder, which goes once done, as the tasks come;
/// or to tidy the folder's open tasks, which stays with its suggestions until they are taken.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiJob {
    pub id: AiJobId,
    pub folder_id: Option<FolderId>,
    /// Organizing for jobs recorded before there were kinds.
    #[serde(default)]
    pub kind: JobKind,
    pub text: JobText,
    pub local_time: LocalTime,
    pub state: JobState,
    /// Why it failed; only when it has.
    pub error: Option<String>,
    /// Only when ready.
    #[serde(default)]
    pub suggestions: Option<Vec<Suggestion>>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One entity change made by a mutation. In JSON it is `{kind, before, after}`, where `before` is
/// `null` for a creation and `after` is `null` for a deletion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Effect {
    Folder(Change<Folder>),
    Task(Change<Box<Task>>),
    Dependency(Link<Dependency>),
    AiJob(Change<Box<AiJob>>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(try_from = "Sides<T>", into = "Sides<T>")]
#[ts(as = "Sides<T>")]
pub enum Change<T: Clone> {
    Created(T),
    Updated { before: T, after: T },
    Deleted(T),
}

/// Dependencies are only ever added or removed, never changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(try_from = "Sides<T>", into = "Sides<T>")]
#[ts(as = "Sides<T>")]
pub enum Link<T: Clone> {
    Added(T),
    Removed(T),
}

/// How `Change` and `Link` look in JSON: `before` is `null` for a creation, `after` for a deletion.
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Sides<T> {
    before: Option<T>,
    after: Option<T>,
}

impl<T: Clone> TryFrom<Sides<T>> for Change<T> {
    type Error = &'static str;

    fn try_from(s: Sides<T>) -> Result<Self, Self::Error> {
        match (s.before, s.after) {
            (None, Some(after)) => Ok(Change::Created(after)),
            (Some(before), Some(after)) => Ok(Change::Updated { before, after }),
            (Some(before), None) => Ok(Change::Deleted(before)),
            (None, None) => Err("before and after are both missing"),
        }
    }
}

impl<T: Clone> From<Change<T>> for Sides<T> {
    fn from(c: Change<T>) -> Self {
        match c {
            Change::Created(after) => Sides { before: None, after: Some(after) },
            Change::Updated { before, after } => Sides { before: Some(before), after: Some(after) },
            Change::Deleted(before) => Sides { before: Some(before), after: None },
        }
    }
}

impl<T: Clone> TryFrom<Sides<T>> for Link<T> {
    type Error = &'static str;

    fn try_from(s: Sides<T>) -> Result<Self, Self::Error> {
        match (s.before, s.after) {
            (None, Some(after)) => Ok(Link::Added(after)),
            (Some(before), None) => Ok(Link::Removed(before)),
            _ => Err("a dependency is either added or removed"),
        }
    }
}

impl<T: Clone> From<Link<T>> for Sides<T> {
    fn from(l: Link<T>) -> Self {
        match l {
            Link::Added(after) => Sides { before: None, after: Some(after) },
            Link::Removed(before) => Sides { before: Some(before), after: None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn validates_text() {
        assert_eq!(from_value::<Title>(json!("  写初稿 ")).unwrap().as_str(), "写初稿");
        assert!(from_value::<Title>(json!("  ")).is_err());
        assert!(from_value::<Title>(json!("一\n二")).is_err());
        assert!(from_value::<Name>(json!("名".repeat(101))).is_err());
        assert!(from_value::<Name>(json!("工作/网站")).is_err());
        assert!(from_value::<Description>(json!("")).is_ok());
        assert!(from_value::<Title>(json!("清屏\u{1b}[2J")).is_err(), "control characters");
        assert!(from_value::<Name>(json!("a\tb")).is_err());
        assert!(from_value::<Description>(json!("- [ ] 一\r\n\t- [ ] 二")).is_ok());
        assert!(from_value::<Description>(json!("\u{9b}2J")).is_err());
    }

    #[test]
    fn validates_dates_and_times() {
        for ok in ["2026-11-02", "2024-02-29"] {
            assert!(from_value::<Date>(json!(ok)).is_ok(), "{ok}");
        }
        for bad in ["2026-2-1", "2026年11月2日", "2025-02-29", "2026-13-01", "2026-11-31", "+026-11-02", "2026/11/02"] {
            assert!(from_value::<Date>(json!(bad)).is_err(), "{bad}");
        }
        assert!(from_value::<Time>(json!("23:59")).is_ok());
        for bad in ["24:00", "9:00", "九点", "1", "12:60", "12-30"] {
            assert!(from_value::<Time>(json!(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn ids_must_be_uuid_v7() {
        assert!(from_value::<TaskId>(json!("019a4b2c-0000-7000-8000-000000000001")).is_ok());
        assert!(from_value::<TaskId>(json!("00000000-0000-0000-0000-000000000000")).is_err());
        assert!(from_value::<TaskId>(json!("9b2e5f7a-3c1d-4e8f-9a6b-2d4c8e1f3a5b")).is_err());
        assert!(from_value::<TaskId>(json!("019a4b2c-0000-7000-0000-000000000001")).is_err(), "wrong variant");
    }

    #[test]
    fn effects_use_before_and_after() {
        let dep =
            json!({ "before": "019a4b2c-0000-7000-8000-000000000001", "after": "019a4b2c-0000-7000-8000-000000000002", "created_at": 1 });
        let added = json!({ "kind": "dependency", "before": null, "after": dep });
        assert_eq!(to_value(from_value::<Effect>(added.clone()).unwrap()).unwrap(), added);
        assert!(from_value::<Effect>(json!({ "kind": "dependency", "before": dep, "after": dep })).is_err());
        assert!(from_value::<Effect>(json!({ "kind": "folder", "before": null, "after": null })).is_err());
    }

    #[test]
    fn due_round_trips_and_rejects_reminder_without_time() {
        let due = json!({ "date": "2026-11-02", "time": "18:00", "remind_before": 60 });
        assert_eq!(to_value(from_value::<Due>(due.clone()).unwrap()).unwrap(), due);
        assert_eq!(to_value(from_value::<Due>(json!({ "date": "2026-11-20" })).unwrap()).unwrap(), json!({ "date": "2026-11-20" }));
        assert!(from_value::<Due>(json!({ "date": "2026-11-20", "remind_before": 0 })).is_err());
        assert!(from_value::<Due>(json!({ "date": "2026-11-20", "time": "08:00", "remind_before": 50000 })).is_err());
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

    #[test]
    fn suggestions_are_told_by_their_fields_and_read_as_kept_before() {
        let id = "019a4b2c-0000-7000-8000-000000000001";
        let was = json!({ "title": "官网", "description": "", "due": null });
        // As tidyings kept them before there were other kinds.
        let edit = json!({ "task": id, "title": "建好官网", "due": { "date": "2026-11-20" }, "after": [], "reason": "", "was": was });
        assert!(matches!(from_value::<Suggestion>(edit.clone()).unwrap(), Suggestion::Edit(e) if e.due.is_some() && e.unlink.is_empty()));
        assert_eq!(to_value(from_value::<Suggestion>(edit.clone()).unwrap()).unwrap(), edit);
        let cleared = json!({ "task": id, "due": null, "after": [], "reason": "", "was": was });
        assert!(matches!(from_value::<Suggestion>(cleared).unwrap(), Suggestion::Edit(e) if e.due == Some(None)));
        let kinds = [
            json!({ "delete": id, "reason": "", "was": was }),
            json!({ "create": id, "title": "新的", "after": [], "before": [id], "reason": "" }),
            json!({ "order": [id], "reason": "" }),
        ];
        for k in kinds {
            assert_eq!(to_value(from_value::<Suggestion>(k.clone()).unwrap()).unwrap(), k);
        }
    }
}
