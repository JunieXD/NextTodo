//! The due dates repeating tasks move on to, as `fixtures/repeat.json` has them; the web app checks
//! the same cases.

use nexttodo_core::model::{Date, Repeat};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    repeat: Repeat,
    due: Date,
    today: Date,
    /// `null` when there is none: past the year 9999.
    next: Option<Date>,
    /// A start date, and where it moves with the due date.
    #[serde(default)]
    start: Option<Date>,
    #[serde(default)]
    next_start: Option<Date>,
}

#[test]
fn repeats_as_the_fixtures_say() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/repeat.json")).unwrap();
    for c in cases {
        assert_eq!(c.repeat.next(&c.due, &c.today).ok(), c.next, "{}", c.name);
        if let (Some(start), Some(next)) = (&c.start, &c.next) {
            assert_eq!(Some(start.shifted(&c.due, next)), c.next_start, "{}", c.name);
        }
    }
}
