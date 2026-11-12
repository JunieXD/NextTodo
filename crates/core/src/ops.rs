//! Mutations: what clients submit to change data. See `docs/api.md`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{
    AiJobId, Date, Description, Due, FolderId, JobKind, JobText, LocalTime, MutationId, Name, SortKey, TaskId, Title, nullable,
};

pub const MAX_OPS: usize = 1000;

/// One or more operations applied together: all of them take effect, or none.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(try_from = "MutationFields")]
#[ts(export)]
pub struct Mutation {
    pub id: MutationId,
    pub ops: Vec<Op>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationFields {
    id: MutationId,
    ops: Vec<Op>,
}

impl TryFrom<MutationFields> for Mutation {
    type Error = String;

    fn try_from(f: MutationFields) -> Result<Self, String> {
        match f.ops.len() {
            0 => Err("改动至少包含一个操作".into()),
            n if n > MAX_OPS => Err(format!("每个改动最多 {MAX_OPS} 个操作")),
            _ => Ok(Mutation { id: f.id, ops: f.ops }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "op", deny_unknown_fields)]
#[ts(export)]
pub enum Op {
    #[serde(rename = "folder.create")]
    FolderCreate {
        id: FolderId,
        name: Name,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        parent_id: Option<FolderId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        sort_key: Option<SortKey>,
    },
    #[serde(rename = "folder.update")]
    FolderUpdate {
        id: FolderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        name: Option<Name>,
        /// `null` moves the folder to the root.
        #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        parent_id: Option<Option<FolderId>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        sort_key: Option<SortKey>,
    },
    #[serde(rename = "folder.delete")]
    FolderDelete { id: FolderId },
    #[serde(rename = "task.create")]
    TaskCreate {
        id: TaskId,
        title: Title,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        folder_id: Option<FolderId>,
        #[serde(default, skip_serializing_if = "is_empty")]
        #[ts(as = "Option<Description>", optional)]
        description: Description,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        start: Option<Date>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        due: Option<Due>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        sort_key: Option<SortKey>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(as = "Option<bool>", optional)]
        done: bool,
    },
    #[serde(rename = "task.update")]
    TaskUpdate {
        id: TaskId,
        /// `null` moves the task to the root.
        #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        folder_id: Option<Option<FolderId>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        title: Option<Title>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        description: Option<Description>,
        /// `null` clears the start date.
        #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        start: Option<Option<Date>>,
        /// `null` clears the due date.
        #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        due: Option<Option<Due>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        sort_key: Option<SortKey>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        done: Option<bool>,
        /// The day on the device that marks a task done: a repeating task's next due date counts
        /// from it, so it must be given for one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        today: Option<Date>,
    },
    #[serde(rename = "task.delete")]
    TaskDelete { id: TaskId },
    #[serde(rename = "dep.add")]
    DepAdd { before: TaskId, after: TaskId },
    #[serde(rename = "dep.remove")]
    DepRemove { before: TaskId, after: TaskId },
    #[serde(rename = "ai_job.create")]
    AiJobCreate {
        id: AiJobId,
        /// Organizing when not given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        kind: Option<JobKind>,
        text: JobText,
        local_time: LocalTime,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        folder_id: Option<FolderId>,
    },
    /// Puts a failed job, or a tidying with suggestions, back in the queue, with its text or folder
    /// changed if given.
    #[serde(rename = "ai_job.retry")]
    AiJobRetry {
        id: AiJobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        text: Option<JobText>,
        /// `null` is the root.
        #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        folder_id: Option<Option<FolderId>>,
    },
    #[serde(rename = "ai_job.delete")]
    AiJobDelete { id: AiJobId },
    #[serde(rename = "undo")]
    Undo { mutation: MutationId },
}

fn is_empty(d: &Description) -> bool {
    d.as_str().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

    const ID: &str = "019a4b2c-0000-7000-8000-000000000001";

    #[test]
    fn tells_null_from_missing() {
        let op: Op = from_value(json!({ "op": "task.update", "id": ID, "due": null })).unwrap();
        let Op::TaskUpdate { due, folder_id, .. } = op else { panic!() };
        assert_eq!(due, Some(None));
        assert_eq!(folder_id, None);
    }

    #[test]
    fn rejects_unknown_fields_and_empty_mutations() {
        assert!(from_value::<Op>(json!({ "op": "folder.create", "id": ID, "name": "x", "parent": ID })).is_err());
        assert!(from_value::<Op>(json!({ "op": "task.archive", "id": ID })).is_err());
        assert!(from_value::<Mutation>(json!({ "id": ID, "ops": [] })).is_err());
        assert!(from_value::<Mutation>(json!({ "id": ID, "ops": [{ "op": "task.delete", "id": ID }] })).is_ok());
    }
}
