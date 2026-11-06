//! Entities and the validated values they are made of.

use serde::{Deserialize, Serialize};
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
            (true, Some(y), Some(m), Some(d)) => {
                let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
                let days = match m {
                    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                    4 | 6 | 9 | 11 => 30,
                    2 if leap => 29,
                    2 => 28,
                    _ => 0,
                };
                (1..=days).contains(&d)
            }
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

/// When a task is due: a date, optionally with a time and a reminder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "DueFields", into = "DueFields")]
#[ts(export, as = "DueFields")]
pub struct Due {
    pub date: Date,
    pub at: Option<DueAt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DueAt {
    pub time: Time,
    /// Minutes before the due time; 0 means at the due time.
    pub remind_before: Option<u32>,
}

pub const MAX_REMIND_BEFORE: u32 = 30 * 24 * 60;

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
}

impl Due {
    pub fn new(date: Date, time: Option<Time>, remind_before: Option<u32>) -> Result<Due, String> {
        if remind_before.is_some_and(|m| m > MAX_REMIND_BEFORE) {
            return Err("提醒最多提前 30 天".into());
        }
        let at = match (time, remind_before) {
            (Some(time), remind_before) => Some(DueAt { time, remind_before }),
            (None, Some(_)) => return Err("设置提醒需要具体的截止时间".into()),
            (None, None) => None,
        };
        Ok(Due { date, at })
    }
}

impl TryFrom<DueFields> for Due {
    type Error = String;

    fn try_from(f: DueFields) -> Result<Self, String> {
        Due::new(f.date, f.time, f.remind_before)
    }
}

impl From<Due> for DueFields {
    fn from(d: Due) -> Self {
        let (time, remind_before) = d.at.map_or((None, None), |at| (Some(at.time), at.remind_before));
        DueFields { date: d.date, time, remind_before }
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

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Waiting => "waiting",
            JobState::Running => "running",
            JobState::Failed => "failed",
            JobState::Ready => "ready",
        }
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for JobState {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for JobState {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match v.as_str()? {
            "waiting" => Ok(JobState::Waiting),
            "running" => Ok(JobState::Running),
            "failed" => Ok(JobState::Failed),
            "ready" => Ok(JobState::Ready),
            _ => Err(rusqlite::types::FromSqlError::InvalidType),
        }
    }
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

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Organize => "organize",
            JobKind::Tidy => "tidy",
        }
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for JobKind {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for JobKind {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match v.as_str()? {
            "organize" => Ok(JobKind::Organize),
            "tidy" => Ok(JobKind::Tidy),
            _ => Err(rusqlite::types::FromSqlError::InvalidType),
        }
    }
}

/// A change AI suggests to one of the folder's open tasks, for the user to take or leave.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Suggestion {
    pub task: TaskId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<Title>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<Description>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due: Option<Due>,
    /// Tasks to put before it.
    pub after: Vec<TaskId>,
    /// What changes and why, in a sentence.
    pub reason: String,
    /// The task as the model saw it: a field changed since is the user's newer word, not to be
    /// overwritten unasked.
    pub was: Was,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Was {
    pub title: Title,
    pub description: Description,
    pub due: Option<Due>,
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
    }
}
