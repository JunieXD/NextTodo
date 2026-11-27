//! Festivals, solar terms, and the days off and made-up working days the State Council sets around
//! public holidays (docs/design/repeat.md#节日和放假). The web app has a TypeScript twin,
//! `apps/web/src/domain/holidays.ts`; both read `terms.json` and `holidays.json` and are checked
//! against `fixtures/holidays.json`.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::lunar::LunarDate;
use crate::model::Date;
use crate::repeat::parts;

/// A day the State Council has set: off, or worked to make up for one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Holiday {
    Off,
    Work,
}

/// A holiday of a year's notice: its days off, `from` to `to`, and the days worked for them.
#[derive(Deserialize)]
struct Arrangement {
    from: Date,
    to: Date,
    work: Vec<Date>,
}

static ARRANGEMENTS: LazyLock<Vec<Arrangement>> =
    LazyLock::new(|| serde_json::from_str(include_str!("holidays.json")).expect("the table is valid"));

/// For each year from 1900 to 2100, the day of the month of its solar terms, two digits each.
static TERMS: LazyLock<Vec<&str>> = LazyLock::new(|| serde_json::from_str(include_str!("terms.json")).expect("the table is valid"));

/// The solar terms in a year, two a month.
const TERM_NAMES: [&str; 24] = [
    "小寒", "大寒", "立春", "雨水", "惊蛰", "春分", "清明", "谷雨", "立夏", "小满", "芒种", "夏至", "小暑", "大暑", "立秋", "处暑", "白露",
    "秋分", "寒露", "霜降", "立冬", "小雪", "大雪", "冬至",
];

impl Date {
    /// The festival on this day; of two, the lunar one, which moves from year to year.
    pub fn festival(&self) -> Option<&'static str> {
        let lunar = || match self.lunar().filter(|l| !l.leap).map(|l| (l.month, l.day))? {
            (1, 1) => Some("春节"),
            (1, 15) => Some("元宵节"),
            (5, 5) => Some("端午节"),
            (7, 7) => Some("七夕"),
            (8, 15) => Some("中秋节"),
            (9, 9) => Some("重阳节"),
            (12, 8) => Some("腊八节"),
            _ => None,
        };
        let eve = || (self.plus(1).lunar() == Some(LunarDate { month: 1, leap: false, day: 1 })).then_some("除夕");
        let solar = || match parts(self) {
            (_, 1, 1) => Some("元旦"),
            (_, 3, 8) => Some("妇女节"),
            (_, 5, 1) => Some("劳动节"),
            (_, 6, 1) => Some("儿童节"),
            (_, 9, 10) => Some("教师节"),
            (_, 10, 1) => Some("国庆节"),
            _ => None,
        };
        lunar().or_else(eve).or_else(solar)
    }

    /// The solar term on this day, 1900–2100.
    pub fn solar_term(&self) -> Option<&'static str> {
        let (y, m, d) = parts(self);
        let days = TERMS.get(y.checked_sub(1900)? as usize)?;
        let i = (2 * m as usize - 2..2 * m as usize).find(|&i| days[2 * i..2 * i + 2].parse() == Ok(d))?;
        Some(TERM_NAMES[i])
    }

    /// Whether this is a day off or a made-up working day, as the State Council has set it.
    pub fn holiday(&self) -> Option<Holiday> {
        ARRANGEMENTS.iter().find_map(|a| {
            if (&a.from..=&a.to).contains(&self) { Some(Holiday::Off) } else { a.work.contains(self).then_some(Holiday::Work) }
        })
    }
}
