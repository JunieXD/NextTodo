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

id_type!(FolderId, TaskId, MutationId, AiJobId, AttachmentId);

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
            pub const ALL: &'static [Self] = &[$($name::$variant),*];

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

/// An enum stored and sent as text, each value as given.
macro_rules! text_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident = $text:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),*
        }

        sql_text_enum!($name { $($variant = $text),* });
    };
}

text_enum!(
    /// A folder's colour from the presets (docs/ui.md#文件夹的颜色和图标).
    ColorPreset {
        Red = "red", Orange = "orange", Amber = "amber", Yellow = "yellow", Lime = "lime", Green = "green",
        Teal = "teal", Cyan = "cyan", Blue = "blue", Indigo = "indigo", Purple = "purple", Pink = "pink",
        Brown = "brown", Gray = "gray",
    }
);

/// A folder's colour: a preset by its name, or one of the user's own, `#rrggbb`
/// (docs/ui.md#文件夹的颜色和图标).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "String", into = "String")]
#[ts(export, untagged)]
pub enum FolderColor {
    Preset(ColorPreset),
    #[ts(type = "`#${string}`")]
    Own([u8; 3]),
}

impl TryFrom<String> for FolderColor {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        if let Some(hex) = s.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())) {
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex digits");
            return Ok(Self::Own([byte(0), byte(2), byte(4)]));
        }
        ColorPreset::ALL.iter().find(|p| p.as_str() == s).map(|&p| Self::Preset(p)).ok_or_else(|| {
            let names: Vec<_> = ColorPreset::ALL.iter().map(|p| p.as_str()).collect();
            format!("颜色要是 {} 之一，或者 # 加六位十六进制数，如 #3478f6", names.join("、"))
        })
    }
}

impl From<FolderColor> for String {
    fn from(c: FolderColor) -> String {
        match c {
            FolderColor::Preset(p) => p.as_str().to_owned(),
            FolderColor::Own([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for FolderColor {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(String::from(*self).into())
    }
}

#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for FolderColor {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Self::try_from(v.as_str()?.to_owned()).map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

text_enum!(
    /// A folder's icon, from the presets (docs/ui.md#文件夹的颜色和图标).
    FolderIcon {
        Briefcase = "briefcase", GraduationCap = "graduation-cap", Book = "book", Pen = "pen", File = "file",
        Laptop = "laptop", Code = "code", Flask = "flask", Calculator = "calculator", Lightbulb = "lightbulb",
        Target = "target", House = "house", Cart = "cart", Utensils = "utensils", Coffee = "coffee", Shirt = "shirt",
        Baby = "baby", Paw = "paw", Heart = "heart", Gift = "gift", Dumbbell = "dumbbell", Bike = "bike",
        HeartPulse = "heart-pulse", Pill = "pill", Wallet = "wallet", PiggyBank = "piggy-bank", Plane = "plane",
        Car = "car", MapPin = "map-pin", Music = "music", Camera = "camera", Palette = "palette", Gamepad = "gamepad",
        Star = "star", Flag = "flag", Rocket = "rocket", Trophy = "trophy", Wrench = "wrench", Leaf = "leaf",
        Users = "users",
    }
);

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

/// The longest a folder name may be, in characters.
pub const MAX_NAME: usize = 100;

text_type!(
    /// Folder name. Without `/`, so that paths name folders.
    Name,
    |s| {
        let s = line(s, "名称", MAX_NAME)?;
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
    /// What was written for AI to organize into tasks: empty when the pictures attached say it all
    /// (docs/ai.md#附件). For a tidying, what to look for, which it needs.
    JobText,
    |s| if s.chars().count() > 10_000 {
        Err("内容不能超过 10000 个字".into())
    } else if s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        Err("内容不能包含控制字符".into())
    } else {
        Ok(s)
    }
);
text_type!(
    /// What the user wrote for AI to keep to when organizing in a folder, and in those inside it
    /// (docs/ai.md#说明). Empty, there is none.
    NoteText,
    |s| {
        let s = s.trim().to_owned();
        if s.chars().count() > 2000 {
            Err("说明不能超过 2000 个字".into())
        } else if s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
            Err("说明不能包含控制字符".into())
        } else {
            Ok(s)
        }
    }
);
text_type!(
    /// The user's local time as written by their device, such as `2026-11-04 16:30 星期三`: what
    /// "tomorrow" means in a text submitted for AI to organize.
    LocalTime,
    |s| line(s, "本地时间", 40)
);
text_type!(
    /// An attachment's name, which is the file's name when it is saved: one line, and not a path.
    FileName,
    |s| {
        let s = line(s, "名称", 200)?;
        if s.contains(['/', '\\']) {
            Err("名称不能包含 / 或 \\".into())
        } else if s == "." || s == ".." {
            Err("名称不能是 . 或 ..".into())
        } else {
            Ok(s)
        }
    }
);
impl FileName {
    /// The name as a file on any system's disk (docs/desktop.md#附件): what Windows, the strictest,
    /// does not allow replaced or changed, and at most 255 bytes, the extension kept. Which file it is
    /// is told by the directory it goes in, named by the attachment's id.
    pub fn on_disk(&self) -> String {
        const MAX: usize = 255;
        let mut name: String = self.0.chars().map(|c| if "<>:\"|?*".contains(c) { '_' } else { c }).collect();
        if name.len() > MAX {
            // An extension longer than this is more likely part of the name.
            let ext = name.rfind('.').filter(|&i| name.len() - i <= 16).map_or(String::new(), |i| name[i..].to_owned());
            let mut end = MAX - ext.len();
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            name = format!("{}{ext}", &name[..end]);
        }
        name.truncate(name.trim_end_matches(['.', ' ']).len());
        if name.is_empty() {
            return "附件".into();
        }
        let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
        let numbered = |prefix: &str| {
            stem.strip_prefix(prefix).is_some_and(|n| {
                let mut digits = n.chars();
                matches!((digits.next(), digits.next()), (Some('0'..='9' | '¹' | '²' | '³'), None))
            })
        };
        if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
            name.insert(0, '_');
        }
        name
    }
}
text_type!(
    /// What kind of file an attachment is, as a MIME type such as `image/png`: lower case, without
    /// parameters.
    MediaType,
    |s| {
        let s = s.trim().to_ascii_lowercase();
        let token = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b));
        match s.split_once('/') {
            Some((kind, sub)) if s.len() <= 100 && token(kind) && token(sub) => Ok(s),
            _ => Err(format!("文件类型“{s}”格式错误，应为“类型/子类型”")),
        }
    }
);
text_type!(
    /// A calendar date `YYYY-MM-DD` in the user's local time.
    Date,
    |s| {
        let digits =
            |r: std::ops::Range<usize>| s.get(r).filter(|p| p.bytes().all(|c| c.is_ascii_digit())).and_then(|p| p.parse::<u32>().ok());
        let valid = match (s.len() == 10 && s.get(4..5) == Some("-") && s.get(7..8) == Some("-"), digits(0..4), digits(5..7), digits(8..10))
        {
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
        let digits =
            |r: std::ops::Range<usize>| s.get(r).filter(|p| p.bytes().all(|c| c.is_ascii_digit())).and_then(|p| p.parse::<u32>().ok());
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

/// A repeating task stays open: done, its due date moves on, until the last time (docs/design.md#重复).
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
/// year is 12 months.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Day,
    Week(Option<Weekdays>),
    Month(Option<Monthdays>),
    LunarMonth(Monthdays),
}

impl Period {
    pub fn unit(self) -> RepeatUnit {
        match self {
            Period::Day => RepeatUnit::Day,
            Period::Week(_) => RepeatUnit::Week,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    until: Option<Date>,
}

impl TryFrom<RepeatFields> for Repeat {
    type Error = &'static str;

    fn try_from(f: RepeatFields) -> Result<Self, Self::Error> {
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
        };
        let lunar = matches!(r.period, Period::LunarMonth(_));
        RepeatFields { every: r.every, unit: r.period.unit(), from: r.from, days, lunar, until: r.until }
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

/// A time of a repeating task: the one due on `due` after it was done `completions` times. The date
/// alone does not tell: done early, from the day it is done, the next can be due on the same day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Occurrence {
    pub due: Date,
    pub completions: u32,
}

/// A time a repeating task was done (docs/design.md#重复): the time `due` on a day, `done` on a day, on
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

/// The time of day and the reminders of a due date, checked: sorted, none twice.
fn timing(time: Option<Time>, reminders: Vec<Remind>) -> Result<(Option<DueAt>, Vec<RemindOn>), String> {
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
        None => return Err("提前多久的提醒需要具体的截止时间".into()),
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
        let (at, remind_on) = timing(time, reminders)?;
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
        let (at, remind_on) = timing(time, reminders)?;
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

/// The largest file an attachment can have: 20 MB.
pub const MAX_FILE_SIZE: u32 = 20 * 1024 * 1024;

/// How many bytes a file has: 1 to `MAX_FILE_SIZE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "u32", into = "u32")]
#[ts(export, type = "number")]
pub struct FileSize(u32);

impl FileSize {
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for FileSize {
    type Error = &'static str;

    fn try_from(n: u32) -> Result<Self, Self::Error> {
        match n {
            0 => Err("文件不能为空"),
            n if n > MAX_FILE_SIZE => Err("文件不能超过 20 MB"),
            n => Ok(FileSize(n)),
        }
    }
}

impl From<FileSize> for u32 {
    fn from(s: FileSize) -> u32 {
        s.0
    }
}

#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for FileSize {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        FileSize::try_from(u32::column_result(v)?).map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for FileSize {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.into())
    }
}

/// What an attachment belongs to: a task, or an AI job until AI has organized it into tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Task(TaskId),
    Job(AiJobId),
}

/// A file attached to a task or an AI job (docs/design.md#附件). The file itself is uploaded
/// separately, under the same id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "AttachmentFields", into = "AttachmentFields")]
#[ts(export, as = "AttachmentFields")]
pub struct Attachment {
    pub id: AttachmentId,
    pub holder: Holder,
    pub name: FileName,
    pub media_type: MediaType,
    pub size: FileSize,
    pub created_at: i64,
}

/// How `Attachment` looks in JSON: exactly one of `task_id` and `job_id`.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "AttachmentFields")]
pub struct AttachmentFields {
    id: AttachmentId,
    task_id: Option<TaskId>,
    job_id: Option<AiJobId>,
    name: FileName,
    #[serde(rename = "type")]
    media_type: MediaType,
    size: FileSize,
    created_at: i64,
}

impl TryFrom<AttachmentFields> for Attachment {
    type Error = &'static str;

    fn try_from(f: AttachmentFields) -> Result<Self, Self::Error> {
        let holder = match (f.task_id, f.job_id) {
            (Some(t), None) => Holder::Task(t),
            (None, Some(j)) => Holder::Job(j),
            _ => return Err("附件要属于一个事项或一条 AI 整理请求"),
        };
        Ok(Attachment { id: f.id, holder, name: f.name, media_type: f.media_type, size: f.size, created_at: f.created_at })
    }
}

impl From<Attachment> for AttachmentFields {
    fn from(a: Attachment) -> Self {
        let (task_id, job_id) = match a.holder {
            Holder::Task(t) => (Some(t), None),
            Holder::Job(j) => (None, Some(j)),
        };
        AttachmentFields { id: a.id, task_id, job_id, name: a.name, media_type: a.media_type, size: a.size, created_at: a.created_at }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Folder {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: Name,
    pub sort_key: SortKey,
    /// None is the default; absent from what was written before folders had them.
    #[serde(default)]
    pub color: Option<FolderColor>,
    #[serde(default)]
    pub icon: Option<FolderIcon>,
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
    /// The day it can start: it waits until then (docs/design.md#开始时间). Not after the due date.
    #[serde(default)]
    pub start: Option<Date>,
    /// The time on the start day it starts; without one, the start of the day. Only with a start date,
    /// and not after the due time on the same day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start_time: Option<Time>,
    /// Due some days after its prerequisites are all done, as `due` then says; it has no other due
    /// date (docs/design.md#前置完成后的截止).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due_after: Option<DueAfter>,
    /// The day its prerequisites were all done, from which `due_after` counts, on the device that did
    /// the last of them: its due date is that, or its start if later, and `due_after.days` on. Only
    /// with `due_after`; none while they are not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unlocked_on: Option<Date>,
    pub due: Option<Due>,
    /// None until a repeating task is first done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completions: Option<Completions>,
    pub sort_key: SortKey,
    /// Who is to do it, by user id in order: in a shared folder, some of the people it is shared
    /// with; none means any of them. Elsewhere none.
    pub assignees: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// The user who added it and who changed it last.
    pub created_by: String,
    pub updated_by: String,
    /// Name of the API token that created the task; `None` when created by the user on a device.
    pub created_via: Option<String>,
    pub updated_via: Option<String>,
}

impl Task {
    /// The time it is due now, should it repeat.
    pub fn occurrence(&self) -> Option<Occurrence> {
        let due = self.due.as_ref().filter(|d| d.repeat.is_some())?;
        Some(Occurrence { due: due.date.clone(), completions: self.completions.as_ref().map_or(0, |c| c.count.get()) })
    }

    /// The reminders of its due date, also while that is still to come after its prerequisites.
    pub fn reminders(&self) -> Vec<Remind> {
        match (&self.due_after, &self.due) {
            (Some(a), _) => a.reminders(),
            (None, Some(d)) => d.reminders(),
            (None, None) => Vec::new(),
        }
    }
}

/// Someone the user shares folders with, or the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Person {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

/// A folder its owner shares with others, who see and change everything in it as the owner does
/// (docs/design.md#共享). One with no members is not shared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Share {
    pub folder_id: FolderId,
    pub owner: String,
    /// User ids in order.
    pub members: Vec<String>,
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
    Edit(Box<TaskEdit>),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due_after: Option<DueAfter>,
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
    /// Those of the device that asked, or asked again, for tasks it is to make with a due date and
    /// no reminders (docs/ai.md#提示词和结果); none when that device has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reminders: Option<DefaultReminders>,
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
    Completion(Link<Completion>),
    Attachment(Change<Attachment>),
    AiJob(Change<Box<AiJob>>),
    Share(Change<Share>),
    /// Someone's name changed: only ever updated.
    Person(Change<Person>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(try_from = "Sides<T>", into = "Sides<T>")]
#[ts(as = "Sides<T>")]
pub enum Change<T: Clone> {
    Created(T),
    Updated { before: T, after: T },
    Deleted(T),
}

/// Dependencies and completion records are only ever added or removed, never changed.
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
            _ => Err("a link is either added or removed"),
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
    fn makes_attachment_names_files() {
        let disk = |s: &str| from_value::<FileName>(json!(s)).unwrap().on_disk();
        assert_eq!(disk("会议 10:30?.png"), "会议 10_30_.png");
        assert_eq!(disk("..."), "附件");
        assert_eq!(disk("说明. "), "说明");
        assert_eq!(disk("con.txt"), "_con.txt");
        assert_eq!(disk("COM1"), "_COM1");
        assert_eq!(disk("COM10.txt"), "COM10.txt");
        assert_eq!(disk("com².log"), "_com².log");
        assert_eq!(disk("LPT¹"), "_LPT¹");
        let long = disk(&format!("{}.pdf", "汉".repeat(100)));
        assert!(long.len() <= 255 && long.ends_with("汉.pdf"), "{long}");
        assert!(from_value::<FileName>(json!("a/b")).is_err());
        assert!(from_value::<FileName>(json!("..")).is_err());
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
