//! Moving a repeating task's due date on when it is done (docs/design.md#重复). The web app has a
//! TypeScript twin, `apps/web/src/domain/repeat.ts`; both are checked against `fixtures/repeat.json`.

use crate::model::{Date, Repeat, RepeatFrom, RepeatUnit, days_in_month};

impl Repeat {
    /// The due date after `due`, the task done `today`: a step on from the start, or as many as it
    /// takes to get past today. An error past the year 9999, which dates don't go beyond.
    pub fn next(self, due: &Date, today: &Date) -> Result<Date, String> {
        let (y, m, d) = parts(match self.from {
            RepeatFrom::Due => due,
            RepeatFrom::Done => today,
        });
        let today = day_number(parts(today));
        // The end of a month stays the end of the month, the 28th or the 31st.
        let month_end = d == days_in_month(y, m);
        let every = self.every.get();
        let step = |k: u32| match self.unit {
            RepeatUnit::Day => civil(day_number((y, m, d)) + i64::from(k * every)),
            RepeatUnit::Week => civil(day_number((y, m, d)) + i64::from(7 * k * every)),
            RepeatUnit::Month => {
                let months = y * 12 + m - 1 + k * every;
                let (y, m) = (months / 12, months % 12 + 1);
                let last = days_in_month(y, m);
                (y, m, if month_end { last } else { d.min(last) })
            }
        };
        let next = (1..).map(step).find(|&n| day_number(n) > today).expect("dates go on");
        Date::try_from(format!("{:04}-{:02}-{:02}", next.0, next.1, next.2)).map_err(|_| "下一次的截止日期超出范围".into())
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
