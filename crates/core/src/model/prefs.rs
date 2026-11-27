//! The settings that go with the account, the same on each of the user's devices
//! (docs/ui/settings.md#跟着账号的设置).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{DefaultReminders, FolderId};

/// What the user has set: anything not set is the app's default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Prefs {
    /// The reminders a task gets as it is given a due date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reminders: Option<DefaultReminders>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub accent: Option<Accent>,
    /// Whether the calendar offers the time grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub time_grid: Option<bool>,
    /// Whether calendars mark days off and made-up working days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub holidays: Option<bool>,
    /// How many of a folder's most recently completed tasks stay in its list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub keep_done: Option<KeepDone>,
    /// The folder the quick add bars put a task in until another is picked, fixed in the settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub add_to: Option<AddTo>,
}

text_enum!(
    /// An accent from the presets (docs/ui.md#颜色).
    AccentPreset { Emerald = "emerald", Lime = "lime", Cyan = "cyan", Blue = "blue", Indigo = "indigo", Purple = "purple", Pink = "pink" }
);

/// The accent colour: a preset by its name, or one of the user's own, `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "String", into = "String")]
#[ts(export, type = "string")]
pub enum Accent {
    Preset(AccentPreset),
    Own([u8; 3]),
}

impl TryFrom<String> for Accent {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        if let Some(hex) = s.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())) {
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex digits");
            return Ok(Self::Own([byte(0), byte(2), byte(4)]));
        }
        AccentPreset::ALL
            .iter()
            .find(|p| p.as_str() == s)
            .map(|&p| Self::Preset(p))
            .ok_or_else(|| "主题色要是预设的一种，或者 # 加六位十六进制数".into())
    }
}

impl From<Accent> for String {
    fn from(a: Accent) -> String {
        match a {
            Accent::Preset(p) => p.as_str().to_owned(),
            Accent::Own([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// How many recently completed tasks stay in a folder's list: 0–20.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "u8", into = "u8")]
#[ts(export, type = "number")]
pub struct KeepDone(u8);

impl TryFrom<u8> for KeepDone {
    type Error = &'static str;

    fn try_from(n: u8) -> Result<Self, Self::Error> {
        if n <= 20 { Ok(KeepDone(n)) } else { Err("保留最近完成的事项最多 20 项") }
    }
}

impl From<KeepDone> for u8 {
    fn from(k: KeepDone) -> u8 {
        k.0
    }
}

/// The folder fixed for the quick add bars; `null` is the root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AddTo {
    pub folder: Option<FolderId>,
}
