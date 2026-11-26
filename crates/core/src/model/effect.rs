//! What a change did, entity by entity, before and after.

use super::{AiJob, Attachment, Completion, Dependency, Folder, Person, Share, Task};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One entity change made by a mutation. In JSON it is `{kind, before, after}`, where `before` is
/// `null` for a creation and `after` is `null` for a deletion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Effect {
    Folder(Change<Folder>),
    Task(Change<Box<Task>>),
    Dependency(Link<Dependency>),
    Completion(Link<Completion>),
    Attachment(Change<Attachment>),
    AiJob(Change<Box<AiJob>>),
    Share(Change<Share>),
    /// Someone's name changed: only ever updated.
    Person(Change<Person>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(try_from = "Sides<T>", into = "Sides<T>")]
#[ts(as = "Sides<T>")]
pub enum Change<T: Clone> {
    Created(T),
    Updated { before: T, after: T },
    Deleted(T),
}

/// Dependencies and completion records are only ever added or removed, never changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(try_from = "Sides<T>", into = "Sides<T>")]
#[ts(as = "Sides<T>")]
pub enum Link<T: Clone> {
    Added(T),
    Removed(T),
}

/// How `Change` and `Link` look in JSON: `before` is `null` for a creation, `after` for a deletion.
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Sides<T> {
    before: Option<T>,
    after: Option<T>,
}

impl<T: Clone> TryFrom<Sides<T>> for Change<T> {
    type Error = &'static str;

    fn try_from(s: Sides<T>) -> Result<Self, Self::Error> {
        match (s.before, s.after) {
            (None, Some(after)) => Ok(Change::Created(after)),
            (Some(before), Some(after)) => Ok(Change::Updated { before, after }),
            (Some(before), None) => Ok(Change::Deleted(before)),
            (None, None) => Err("before and after are both missing"),
        }
    }
}

impl<T: Clone> From<Change<T>> for Sides<T> {
    fn from(c: Change<T>) -> Self {
        match c {
            Change::Created(after) => Sides { before: None, after: Some(after) },
            Change::Updated { before, after } => Sides { before: Some(before), after: Some(after) },
            Change::Deleted(before) => Sides { before: Some(before), after: None },
        }
    }
}

impl<T: Clone> TryFrom<Sides<T>> for Link<T> {
    type Error = &'static str;

    fn try_from(s: Sides<T>) -> Result<Self, Self::Error> {
        match (s.before, s.after) {
            (None, Some(after)) => Ok(Link::Added(after)),
            (Some(before), None) => Ok(Link::Removed(before)),
            _ => Err("a link is either added or removed"),
        }
    }
}

impl<T: Clone> From<Link<T>> for Sides<T> {
    fn from(l: Link<T>) -> Self {
        match l {
            Link::Added(after) => Sides { before: None, after: Some(after) },
            Link::Removed(before) => Sides { before: Some(before), after: None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn effects_use_before_and_after() {
        let dep =
            json!({ "before": "019a4b2c-0000-7000-8000-000000000001", "after": "019a4b2c-0000-7000-8000-000000000002", "created_at": 1 });
        let added = json!({ "kind": "dependency", "before": null, "after": dep });
        assert_eq!(to_value(from_value::<Effect>(added.clone()).unwrap()).unwrap(), added);
        assert!(from_value::<Effect>(json!({ "kind": "dependency", "before": dep, "after": dep })).is_err());
        assert!(from_value::<Effect>(json!({ "kind": "folder", "before": null, "after": null })).is_err());
    }
}
