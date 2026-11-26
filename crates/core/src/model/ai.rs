//! AI jobs and the suggestions they make.

use super::{
    AiJobId, DefaultReminders, Description, Due, DueAfter, FolderColor, FolderIcon, FolderId, JobText, LocalTime, Remind, RemindAt, TaskId,
    Title, nullable,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobState {
    Waiting,
    Running,
    Failed,
    /// A tidying's suggestions are there to be taken.
    Ready,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobKind {
    /// Organizing the text into tasks.
    #[default]
    Organize,
    /// Suggesting changes to the folder's open tasks, the text saying what to look for.
    Tidy,
}

sql_text_enum!(JobState { Waiting = "waiting", Running = "running", Failed = "failed", Ready = "ready" });
sql_text_enum!(JobKind { Organize = "organize", Tidy = "tidy" });

/// What AI suggests doing in the folder it tidies, for the user to take or leave part by part. Each
/// kind is told by a field only it has (`task`, `delete`, `create`, `order`, `style`), with no tag:
/// changes to a task came first, and are kept as they were then.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum Suggestion {
    Edit(Box<TaskEdit>),
    Delete(TaskDeletion),
    Create(NewTask),
    Order(Reorder),
    Style(Restyle),
}

/// Changes to a task, the fields given set and the dependencies given added or removed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskEdit {
    pub task: TaskId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<Title>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<Description>,
    /// `null` clears the due date.
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due: Option<Option<Due>>,
    /// Reminders by the start and at times of their own, all of each kind, in place of those it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start_reminders: Option<Vec<Remind>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub remind_at: Option<Vec<RemindAt>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub done: Option<bool>,
    /// A folder inside the one tidied, to move it into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder: Option<FolderId>,
    /// Tasks to put before it.
    pub after: Vec<TaskId>,
    /// Tasks before it now, to be so no more; none in suggestions made before there were these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlink: Vec<TaskId>,
    /// What changes and why, in a sentence.
    pub reason: String,
    /// The task as the model saw it: a field changed since is the user's newer word, not to be
    /// overwritten unasked.
    pub was: Was,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskDeletion {
    pub delete: TaskId,
    pub reason: String,
    pub was: Was,
}

/// A task to add to the folder tidied, under the id it is to have.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewTask {
    pub create: TaskId,
    pub title: Title,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<Description>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due: Option<Due>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<RemindAt>>", optional)]
    pub remind_at: Vec<RemindAt>,
    /// Tasks to put before it, new ones among them.
    pub after: Vec<TaskId>,
    /// Tasks already there to put after it.
    pub before: Vec<TaskId>,
    pub reason: String,
}

/// Some of the folder's open tasks in a new order among themselves, in the places they hold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Reorder {
    pub order: Vec<TaskId>,
    pub reason: String,
}

/// A colour or an icon for the folder tidied or one inside it; `null` is the default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Restyle {
    pub style: FolderId,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<Option<FolderColor>>,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<Option<FolderIcon>>,
    pub reason: String,
    /// The folder's as the model saw them.
    pub was: FolderLook,
}

/// A folder's colour and icon, `None` the default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderLook {
    pub color: Option<FolderColor>,
    pub icon: Option<FolderIcon>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Was {
    pub title: Title,
    pub description: Description,
    pub due: Option<Due>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due_after: Option<DueAfter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Remind>>", optional)]
    pub start_reminders: Vec<Remind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<RemindAt>>", optional)]
    pub remind_at: Vec<RemindAt>,
}

/// Text submitted for AI to organize into tasks in a folder, which goes once done, as the tasks come;
/// or to tidy the folder's open tasks, which stays with its suggestions until they are taken.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiJob {
    pub id: AiJobId,
    pub folder_id: Option<FolderId>,
    /// Organizing for jobs recorded before there were kinds.
    #[serde(default)]
    pub kind: JobKind,
    pub text: JobText,
    pub local_time: LocalTime,
    /// Those of the device that asked, or asked again, for tasks it is to make with a due date and
    /// no reminders (docs/ai.md#提示词和结果); none when that device has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reminders: Option<DefaultReminders>,
    pub state: JobState,
    /// Why it failed; only when it has.
    pub error: Option<String>,
    /// Only when ready.
    #[serde(default)]
    pub suggestions: Option<Vec<Suggestion>>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn suggestions_are_told_by_their_fields_and_read_as_kept_before() {
        let id = "019a4b2c-0000-7000-8000-000000000001";
        let was = json!({ "title": "官网", "description": "", "due": null });
        // As tidyings kept them before there were other kinds.
        let edit = json!({ "task": id, "title": "建好官网", "due": { "date": "2026-11-20" }, "after": [], "reason": "", "was": was });
        assert!(matches!(from_value::<Suggestion>(edit.clone()).unwrap(), Suggestion::Edit(e) if e.due.is_some() && e.unlink.is_empty()));
        assert_eq!(to_value(from_value::<Suggestion>(edit.clone()).unwrap()).unwrap(), edit);
        let cleared = json!({ "task": id, "due": null, "after": [], "reason": "", "was": was });
        assert!(matches!(from_value::<Suggestion>(cleared).unwrap(), Suggestion::Edit(e) if e.due == Some(None)));
        let kinds = [
            json!({ "delete": id, "reason": "", "was": was }),
            json!({ "create": id, "title": "新的", "after": [], "before": [id], "reason": "" }),
            json!({ "order": [id], "reason": "" }),
        ];
        for k in kinds {
            assert_eq!(to_value(from_value::<Suggestion>(k.clone()).unwrap()).unwrap(), k);
        }
    }
}
