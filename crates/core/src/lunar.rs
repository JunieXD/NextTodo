//! The Chinese lunar calendar (docs/design.md#农历), for repeats that go by it and the lunar dates
//! shown beside days. The web app has a TypeScript twin, `apps/web/src/domain/lunar.ts`; both read
//! `lunar.json` and are checked against `fixtures/lunar.json`.

use std::fmt;
use std::sync::LazyLock;

use crate::model::Date;
use crate::repeat::{day_number, parts};

/// A lunar month: its first day, as days since 1970-01-01, how many days it has, and which it is.
struct Entry {
    start: i64,
    len: u32,
    year: u32,
    number: u32,
    leap: bool,
}

/// Every month of the lunar years 1900–2100, in order. `lunar.json` has a number for each year: bit
/// `i` of its lowest 13 is set when the year's month `i` (from 0, a leap month counted where it falls)
/// has 30 days rather than 29; the next 4 bits are the month a leap month follows, 0 for none; the
/// rest, how many days after 21 January the year begins.
static MONTHS: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    let years = include_str!("lunar.json").trim().trim_start_matches('[').trim_end_matches(']').split(',');
    let mut months = Vec::new();
    for (year, bits) in (1900..).zip(years) {
        let bits: u32 = bits.trim().parse().expect("the table is valid");
        let leap = bits >> 13 & 0xf;
        let mut start = day_number((year, 1, 21)) + i64::from(bits >> 17);
        for i in 0..12 + u32::from(leap != 0) {
            let (number, is_leap) = if leap == 0 || i < leap { (i + 1, false) } else { (i, i == leap) };
            let len = if bits & 1 << i != 0 { 30 } else { 29 };
            months.push(Entry { start, len, year, number, leap: is_leap });
            start += i64::from(len);
        }
    }
    months
});

/// A month the table has.
#[derive(Clone, Copy)]
pub(crate) struct Month(usize);

impl Month {
    /// The month day `n` (days since 1970-01-01) is in.
    pub(crate) fn of(n: i64) -> Option<Month> {
        let i = MONTHS.partition_point(|m| m.start <= n).checked_sub(1)?;
        (n < MONTHS[i].start + i64::from(MONTHS[i].len)).then_some(Month(i))
    }

    /// The month `count` months on, a leap month counted too; or, `by_year`, the month of the same
    /// name `count / 12` years on, never a leap one.
    pub(crate) fn on(self, count: u32, by_year: bool) -> Option<Month> {
        if count == 0 {
            return Some(self);
        }
        if !by_year {
            let i = self.0 + count as usize;
            return (i < MONTHS.len()).then_some(Month(i));
        }
        let (year, number) = (MONTHS[self.0].year + count / 12, MONTHS[self.0].number);
        MONTHS.iter().position(|m| m.year == year && m.number == number && !m.leap).map(Month)
    }

    /// Its day `d`, 1–30, or its last if it has no such day: as days since 1970-01-01.
    pub(crate) fn day(self, d: u32) -> i64 {
        let m = &MONTHS[self.0];
        m.start + i64::from(d.min(m.len)) - 1
    }
}

/// A day of the lunar calendar: 八月十五, 闰六月初一.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LunarDate {
    /// 1–12.
    pub month: u32,
    pub leap: bool,
    /// 1–30.
    pub day: u32,
}

impl Date {
    /// This day in the lunar calendar, if the table has it.
    pub fn lunar(&self) -> Option<LunarDate> {
        let n = day_number(parts(self));
        let Month(i) = Month::of(n)?;
        let m = &MONTHS[i];
        Some(LunarDate { month: m.number, leap: m.leap, day: u32::try_from(n - m.start).expect("in the month") + 1 })
    }
}

/// The first day from `from` on that is day `day` (1–30, or its month's last if it has fewer) of lunar
/// month `month` (1–12, and whether a leap one), or of any month. None past the table.
pub fn next(month: Option<(u32, bool)>, day: u32, from: &Date) -> Option<Date> {
    let start = day_number(parts(from));
    let Month(first) = Month::of(start)?;
    let n = (first..MONTHS.len())
        .filter(|&i| month.is_none_or(|(number, leap)| MONTHS[i].number == number && MONTHS[i].leap == leap))
        .map(|i| Month(i).day(day))
        .find(|&n| n >= start)?;
    Some(from.plus(u32::try_from(n - start).expect("not before `from`")))
}

/// 正月, 二月 … 十月, 冬月, 腊月.
pub fn month_name(month: u32) -> &'static str {
    ["正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "冬月", "腊月"][month as usize - 1]
}

/// 初一 … 初十, 十一 … 二十, 廿一 … 廿九, 三十.
pub fn day_name(day: u32) -> String {
    const DIGITS: [&str; 10] = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match day {
        10 => "初十".to_owned(),
        20 => "二十".to_owned(),
        30 => "三十".to_owned(),
        d => format!("{}{}", ["初", "十", "廿"][d as usize / 10], DIGITS[d as usize % 10]),
    }
}

impl fmt::Display for LunarDate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}{}", if self.leap { "闰" } else { "" }, month_name(self.month), day_name(self.day))
    }
}
