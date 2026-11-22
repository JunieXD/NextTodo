//! Text as `fixtures/text.json` has it: what the web app sends once it has checked it, the server
//! takes as it is; it turns down what the web app does, saying the same, and what the web app cleans,
//! as `server_error` says.

use nexttodo_core::model::{Description, FileName, JobText, Name, NoteText, Title};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    kind: String,
    input: String,
    times: Option<usize>,
    /// What the web app makes of `input`, which may drop what the server would turn down; `input`
    /// when not given.
    value: Option<String>,
    error: Option<String>,
    /// What the server says of `input` as it is, which the web app cleans first; `error` when not
    /// given.
    server_error: Option<String>,
}

fn check<T: TryFrom<String, Error = String> + Into<String>>(s: String) -> Result<String, String> {
    T::try_from(s).map(Into::into)
}

#[test]
fn text_is_checked_as_the_fixtures_say() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../fixtures/text.json")).unwrap();
    for c in cases {
        let input = c.input.repeat(c.times.unwrap_or(1));
        let check = match c.kind.as_str() {
            "title" => check::<Title>,
            "folder_name" => check::<Name>,
            "description" => check::<Description>,
            "file_name" => check::<FileName>,
            "job_text" => check::<JobText>,
            "note_text" => check::<NoteText>,
            kind => panic!("{}: no kind {kind}", c.name),
        };
        if let Some(error) = c.server_error.or(c.error.clone()) {
            assert_eq!(check(input.clone()), Err(error), "{}", c.name);
        }
        if c.error.is_none() {
            let value = c.value.unwrap_or(input);
            assert_eq!(check(value.clone()), Ok(value), "{}", c.name);
        }
    }
}
