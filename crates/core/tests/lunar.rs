//! Days in the lunar calendar, as `fixtures/lunar.json` has them; the web app checks the same cases.

use nexttodo_core::model::Date;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    date: Date,
    /// `null` outside the table.
    lunar: Option<String>,
}

#[test]
fn days_are_in_the_lunar_calendar_as_the_fixtures_say() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/lunar.json")).unwrap();
    for c in cases {
        assert_eq!(c.date.lunar().map(|d| d.to_string()), c.lunar, "{}", c.date.as_str());
    }
}
