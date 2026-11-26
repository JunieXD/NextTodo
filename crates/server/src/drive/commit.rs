//! A commit as it is kept in the drive: `<generation>/log/<number>.json` (docs/local.md#网盘里的文件).

use nexttodo_core::model::{AttachmentId, Change, Effect, FileSize, FolderId, MediaType, MutationId, NoteText};
use nexttodo_core::ops::Op;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How this version of the app writes commits, and the newest it reads: one more each time what an
/// older version would not read changes, the operations and what they did included
/// (docs/local.md#格式版本). Every format before it is still read. 2: a repeat may end or go by the lunar
/// calendar, a folder has a colour and an icon, and each time a repeating task is done is recorded. 3:
/// several reminders. 4: a time of a repeating task done again keeps a record of each. 5: a folder's
/// colour may be one of the user's own, and there are more presets. 6: an AI job has the asking device's
/// default reminders, and a task may be due some days after its prerequisites are done. 7: a task may be
/// given up. 8: a tidying may suggest a folder's colour and icon.
pub(crate) const FORMAT: i64 = 8;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Commit {
    /// Tells this commit from another written at the same number: whose it is when the answer to
    /// writing it was lost.
    pub id: Uuid,
    pub format: i64,
    pub items: Vec<Item>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Item {
    Mutation(Made),
    Note(Note),
    Purge(Vec<Cleared>),
}

impl Item {
    /// The attachments it adds, whose files go into the drive before it.
    pub(crate) fn attached(&self) -> impl Iterator<Item = AttachmentId> + '_ {
        let effects = match self {
            Item::Mutation(m) => m.effects.as_slice(),
            Item::Note(_) | Item::Purge(_) => &[],
        };
        effects.iter().filter_map(|e| match e {
            Effect::Attachment(Change::Created(a)) => Some(a.id),
            _ => None,
        })
    }
}

/// A file cleared, as it was recorded: every device keeps the same record of it, and so knows not to
/// use its id again (docs/local.md#附件).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Cleared {
    pub id: AttachmentId,
    #[serde(rename = "type")]
    pub media_type: MediaType,
    pub size: FileSize,
    pub created_at: i64,
    pub unused_at: i64,
}

/// A mutation as it was made: what it did, not to be done again.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Made {
    pub id: MutationId,
    /// When, on the device that made it.
    pub at: i64,
    /// The API token's name, for one made with one.
    pub via: Option<String>,
    pub ops: Vec<Op>,
    pub effects: Vec<Effect>,
}

/// A note for AI organizing, put in place of the one for its folder (docs/ai.md#说明).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Note {
    pub folder_id: Option<FolderId>,
    pub text: NoteText,
    pub at: i64,
}

/// Just the id and format, read first: items of a newer format may not parse.
#[derive(Deserialize)]
pub(crate) struct Head {
    pub id: Uuid,
    pub format: i64,
}

pub(crate) fn path(generation: &str, number: i64) -> String {
    format!("{generation}/log/{number}.json")
}

/// Where attachment `id`'s file is kept.
pub(crate) fn file_path(generation: &str, id: AttachmentId) -> String {
    format!("{generation}/files/{id}")
}
