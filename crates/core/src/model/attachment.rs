//! Files attached to tasks and AI jobs.

use super::{AiJobId, AttachmentId, TaskId, line};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

text_type!(
    /// An attachment's name, which is the file's name when it is saved: one line, and not a path.
    FileName,
    |s| {
        let s = line(s, "名称", 200)?;
        if s.contains(['/', '\\']) {
            Err("名称不能包含 / 或 \\".into())
        } else if s == "." || s == ".." {
            Err("名称不能是 . 或 ..".into())
        } else {
            Ok(s)
        }
    }
);

impl FileName {
    /// The name as a file on any system's disk (docs/desktop.md#附件): what Windows, the strictest,
    /// does not allow replaced or changed, and at most 255 bytes, the extension kept. Which file it is
    /// is told by the directory it goes in, named by the attachment's id.
    pub fn on_disk(&self) -> String {
        const MAX: usize = 255;
        let mut name: String = self.0.chars().map(|c| if "<>:\"|?*".contains(c) { '_' } else { c }).collect();
        if name.len() > MAX {
            // An extension longer than this is more likely part of the name.
            let ext = name.rfind('.').filter(|&i| name.len() - i <= 16).map_or(String::new(), |i| name[i..].to_owned());
            let mut end = MAX - ext.len();
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            name = format!("{}{ext}", &name[..end]);
        }
        name.truncate(name.trim_end_matches(['.', ' ']).len());
        if name.is_empty() {
            return "附件".into();
        }
        let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
        let numbered = |prefix: &str| {
            stem.strip_prefix(prefix).is_some_and(|n| {
                let mut digits = n.chars();
                matches!((digits.next(), digits.next()), (Some('0'..='9' | '¹' | '²' | '³'), None))
            })
        };
        if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
            name.insert(0, '_');
        }
        name
    }
}

text_type!(
    /// What kind of file an attachment is, as a MIME type such as `image/png`: lower case, without
    /// parameters.
    MediaType,
    |s| {
        let s = s.trim().to_ascii_lowercase();
        let token = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b));
        match s.split_once('/') {
            Some((kind, sub)) if s.len() <= 100 && token(kind) && token(sub) => Ok(s),
            _ => Err(format!("文件类型“{s}”格式错误，应为“类型/子类型”")),
        }
    }
);

/// The largest file an attachment can have: 20 MB.
pub const MAX_FILE_SIZE: u32 = 20 * 1024 * 1024;

/// How many bytes a file has: 1 to `MAX_FILE_SIZE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "u32", into = "u32")]
#[ts(export, type = "number")]
pub struct FileSize(u32);

impl FileSize {
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for FileSize {
    type Error = &'static str;

    fn try_from(n: u32) -> Result<Self, Self::Error> {
        match n {
            0 => Err("文件不能为空"),
            n if n > MAX_FILE_SIZE => Err("文件不能超过 20 MB"),
            n => Ok(FileSize(n)),
        }
    }
}

impl From<FileSize> for u32 {
    fn from(s: FileSize) -> u32 {
        s.0
    }
}

#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for FileSize {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        FileSize::try_from(u32::column_result(v)?).map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for FileSize {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.into())
    }
}

/// What an attachment belongs to: a task, or an AI job until AI has organized it into tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Task(TaskId),
    Job(AiJobId),
}

/// A file attached to a task or an AI job (docs/design/attachments.md). The file itself is uploaded
/// separately, under the same id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(try_from = "AttachmentFields", into = "AttachmentFields")]
#[ts(export, as = "AttachmentFields")]
pub struct Attachment {
    pub id: AttachmentId,
    pub holder: Holder,
    pub name: FileName,
    pub media_type: MediaType,
    pub size: FileSize,
    pub created_at: i64,
}

/// How `Attachment` looks in JSON: exactly one of `task_id` and `job_id`.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "AttachmentFields")]
pub struct AttachmentFields {
    id: AttachmentId,
    task_id: Option<TaskId>,
    job_id: Option<AiJobId>,
    name: FileName,
    #[serde(rename = "type")]
    media_type: MediaType,
    size: FileSize,
    created_at: i64,
}

impl TryFrom<AttachmentFields> for Attachment {
    type Error = &'static str;

    fn try_from(f: AttachmentFields) -> Result<Self, Self::Error> {
        let holder = match (f.task_id, f.job_id) {
            (Some(t), None) => Holder::Task(t),
            (None, Some(j)) => Holder::Job(j),
            _ => return Err("附件要属于一个事项或一条 AI 整理请求"),
        };
        Ok(Attachment { id: f.id, holder, name: f.name, media_type: f.media_type, size: f.size, created_at: f.created_at })
    }
}

impl From<Attachment> for AttachmentFields {
    fn from(a: Attachment) -> Self {
        let (task_id, job_id) = match a.holder {
            Holder::Task(t) => (Some(t), None),
            Holder::Job(j) => (None, Some(j)),
        };
        AttachmentFields { id: a.id, task_id, job_id, name: a.name, media_type: a.media_type, size: a.size, created_at: a.created_at }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

    #[test]
    fn makes_attachment_names_files() {
        let disk = |s: &str| from_value::<FileName>(json!(s)).unwrap().on_disk();
        assert_eq!(disk("会议 10:30?.png"), "会议 10_30_.png");
        assert_eq!(disk("..."), "附件");
        assert_eq!(disk("说明. "), "说明");
        assert_eq!(disk("con.txt"), "_con.txt");
        assert_eq!(disk("COM1"), "_COM1");
        assert_eq!(disk("COM10.txt"), "COM10.txt");
        assert_eq!(disk("com².log"), "_com².log");
        assert_eq!(disk("LPT¹"), "_LPT¹");
        let long = disk(&format!("{}.pdf", "汉".repeat(100)));
        assert!(long.len() <= 255 && long.ends_with("汉.pdf"), "{long}");
        assert!(from_value::<FileName>(json!("a/b")).is_err());
        assert!(from_value::<FileName>(json!("..")).is_err());
    }
}
