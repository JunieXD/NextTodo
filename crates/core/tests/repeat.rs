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
}

#[test]
fn repeats_as_the_fixtures_say() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/repeat.json")).unwrap();
    for c in cases {
        assert_eq!(c.repeat.next(&c.due, &c.today).ok(), c.next, "{}", c.name);
    }
}
