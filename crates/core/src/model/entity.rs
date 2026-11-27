//! Folders, tasks, the people folders are shared with, and dependencies.

use super::{
    Completions, Date, Description, Due, DueAfter, FolderColor, FolderIcon, FolderId, Name, Occurrence, Remind, RemindAt, SortKey, TaskId,
    Time, Title,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Folder {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: Name,
    pub sort_key: SortKey,
    /// None is the default; absent from what was written before folders had them.
    #[serde(default)]
    pub color: Option<FolderColor>,
    #[serde(default)]
    pub icon: Option<FolderIcon>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Task {
    pub id: TaskId,
    pub number: i64,
    pub folder_id: Option<FolderId>,
    pub title: Title,
    pub description: Description,
    /// When it ended: done, or given up if `dropped`.
    pub done_at: Option<i64>,
    /// Given up rather than done (docs/design.md#放弃); only once it has ended.
    #[serde(default)]
    pub dropped: bool,
    /// The day it can start: it waits until then (docs/design/dates.md#开始时间). Not after the due date.
    #[serde(default)]
    pub start: Option<Date>,
    /// The time on the start day it starts; without one, the start of the day. Only with a start date,
    /// and not after the due time on the same day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start_time: Option<Time>,
    /// Due some days after its prerequisites are all done, as `due` then says; it has no other due
    /// date (docs/design/dates.md#前置完成后的截止).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub due_after: Option<DueAfter>,
    /// The day its prerequisites were all done, from which `due_after` counts, on the device that did
    /// the last of them: its due date is that, or its start if later, and `due_after.days` on. Only
    /// with `due_after`; none while they are not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unlocked_on: Option<Date>,
    pub due: Option<Due>,
    /// Reminders by the start, in the order `start_reminders` gives: only with a start, those so long
    /// before with its time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Remind>>", optional)]
    pub start_reminders: Vec<Remind>,
    /// Reminders at times of their own, the first first, none twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<RemindAt>>", optional)]
    pub remind_at: Vec<RemindAt>,
    /// None until a repeating task is first done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completions: Option<Completions>,
    pub sort_key: SortKey,
    /// Who is to do it, by user id in order: in a shared folder, some of the people it is shared
    /// with; none means any of them. Elsewhere none.
    pub assignees: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// The user who added it and who changed it last.
    pub created_by: String,
    pub updated_by: String,
    /// Name of the API token that created the task; `None` when created by the user on a device.
    pub created_via: Option<String>,
    pub updated_via: Option<String>,
}

impl Task {
    /// The time it is due now, should it repeat.
    pub fn occurrence(&self) -> Option<Occurrence> {
        let due = self.due.as_ref().filter(|d| d.repeat.is_some())?;
        Some(Occurrence { due: due.date.clone(), completions: self.completions.as_ref().map_or(0, |c| c.count.get()) })
    }

    /// Keeps the reminders by the start that have something left to go by: none without a start, those
    /// so long before only with its time (docs/design/dates.md#提醒).
    pub fn fit_start_reminders(&mut self) {
        let (start, timed) = (self.start.is_some(), self.start_time.is_some());
        self.start_reminders.retain(|r| start && (timed || matches!(r, Remind::On(_))));
    }

    /// How many reminders it has, of all kinds.
    pub fn reminder_count(&self) -> usize {
        self.reminders().len() + self.start_reminders.len() + self.remind_at.len()
    }

    /// The reminders of its due date, also while that is still to come after its prerequisites.
    pub fn reminders(&self) -> Vec<Remind> {
        match (&self.due_after, &self.due) {
            (Some(a), _) => a.reminders(),
            (None, Some(d)) => d.reminders(),
            (None, None) => Vec::new(),
        }
    }
}

/// Someone the user shares folders with, or the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Person {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

/// A folder its owner shares with others, who see and change everything in it as the owner does
/// (docs/design/sharing.md). One with no members is not shared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Share {
    pub folder_id: FolderId,
    pub owner: String,
    /// User ids in order.
    pub members: Vec<String>,
}

/// `before` must be done before `after` can start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Dependency {
    pub before: TaskId,
    pub after: TaskId,
    pub created_at: i64,
}
