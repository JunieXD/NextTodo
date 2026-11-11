//! Moving a repeating task's due date on when it is done (docs/design.md#重复). The web app has a
//! TypeScript twin, `apps/web/src/domain/repeat.ts`; both are checked against `fixtures/repeat.json`.

use crate::model::{Date, Period, Repeat, RepeatFrom, days_in_month};

impl Repeat {
    /// The due date after `due`, the task done `today`: a step on from the start, or as many as it
    /// takes to get past today. On some days of the week or month, a step is to the next of them: later
    /// that week or month, or in the first one `every` on. An error past the year 9999, which dates
    /// don't go beyond.
    pub fn next(self, due: &Date, today: &Date) -> Result<Date, String> {
        let (y, m, d) = parts(match self.from {
            RepeatFrom::Due => due,
            RepeatFrom::Done => today,
        });
        let start = day_number((y, m, d));
        let today = day_number(parts(today));
        let every = self.every.get();
        // The day `d` of the month `k` months on, or its last day if it has none.
        let in_month = |k: u32, d: u32| {
            let months = y * 12 + m - 1 + k;
            let (y, m) = (months / 12, months % 12 + 1);
            day_number((y, m, d.min(days_in_month(y, m))))
        };
        let next = match self.period {
            Period::Week(Some(days)) => {
                // 1970-01-01 was a Thursday.
                let monday = start - (start + 3).rem_euclid(7);
                (0..)
                    .flat_map(|w: i64| (0..7).filter(move |&i| days.has(i)).map(move |i| monday + 7 * w * i64::from(every) + i64::from(i)))
                    .find(|&n| n > start && n > today)
            }
            Period::Month(Some(days)) => (0..)
                .flat_map(|k: u32| (1..=31).filter(move |&d| days.has(d)).map(move |d| in_month(k * every, d)))
                .find(|&n| n > start && n > today),
            period => (1..)
                .map(|k: u32| match period {
                    Period::Day => start + i64::from(k * every),
                    Period::Week(_) => start + i64::from(7 * k * every),
                    // The end of a month stays the end of the month, the 28th or the 31st.
                    Period::Month(_) => in_month(k * every, if d == days_in_month(y, m) { 31 } else { d }),
                })
                .find(|&n| n > today),
        };
        let (y, m, d) = civil(next.expect("dates go on"));
        Date::try_from(format!("{y:04}-{m:02}-{d:02}")).map_err(|_| "下一次的截止日期超出范围".into())
    }
}

/// Year, month and day of a date, which is valid.
fn parts(d: &Date) -> (u32, u32, u32) {
    let n = |r: std::ops::Range<usize>| d.as_str()[r].parse().expect("a valid date");
    (n(0..4), n(5..7), n(8..10))
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn day_number((y, m, d): (u32, u32, u32)) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// The date `days` after 1970-01-01, back again (`civil_from_days`).
fn civil(days: i64) -> (u32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = (mp + 2) % 12 + 1;
    let y = yoe + era * 400 + i64::from(m <= 2);
    let int = |v: i64| u32::try_from(v).expect("dates after year 0");
    (int(y), int(m), int(d))
}
