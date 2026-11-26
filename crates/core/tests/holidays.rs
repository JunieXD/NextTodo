//! Festivals, solar terms and holidays, as `fixtures/holidays.json` has them; the web app checks the
//! same cases.

use nexttodo_core::holidays::Holiday;
use nexttodo_core::model::Date;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    date: Date,
    festival: Option<String>,
    term: Option<String>,
    holiday: Option<String>,
}

#[test]
fn days_have_the_festivals_terms_and_holidays_the_fixtures_say() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/holidays.json")).unwrap();
    for c in cases {
        let holiday = c.date.holiday().map(|h| if h == Holiday::Off { "off" } else { "work" });
        assert_eq!(
            (c.date.festival(), c.date.solar_term(), holiday),
            (c.festival.as_deref(), c.term.as_deref(), c.holiday.as_deref()),
            "{}",
            c.date.as_str()
        );
    }
}
