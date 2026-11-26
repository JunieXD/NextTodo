//! Entities and the validated values they are made of.

#[macro_use]
mod macros;

mod ai;
mod attachment;
mod due;
mod effect;
mod entity;
mod repeat;

pub use self::ai::*;
pub use self::attachment::*;
pub use self::due::*;
pub use self::effect::*;
pub use self::entity::*;
pub use self::repeat::*;
pub use crate::sort_key::SortKey;
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use uuid::Uuid;

id_type!(FolderId, TaskId, MutationId, AiJobId, AttachmentId);

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

/// Tells a missing field (`None`) from an explicit `null` (`Some(None)`).
pub(crate) fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
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

/// How this version of NextTodo writes backups, and the newest it reads (docs/local.md#格式版本).
pub const BACKUP_FORMAT: i64 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

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
}
