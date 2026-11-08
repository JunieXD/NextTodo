//! Text output: short Chinese lines meant for people and agents alike. See `docs/agent.md`.

use std::collections::{BTreeMap, HashMap};

use nexttodo_core::board::{Board, Snapshot, Status};
use nexttodo_core::model::{AiJob, Change, Date, Due, Effect, FolderId, JobState, Link, Repeat, RepeatFrom, RepeatUnit, Task, TaskId};
use serde_json::{Value, json};

/// How much of a title is shown where a task is only mentioned, not listed.
const SHORT: usize = 30;

pub fn status_tag(s: Status) -> String {
    match s {
        Status::Ready => "[可做]".into(),
        Status::Waiting(n) => format!("[等 {n} 项]"),
        Status::Done => "[已完成]".into(),
    }
}

/// `2026-11-20 18:00`, with how it repeats: `2026-11-20，每周`.
pub fn due(d: &Due) -> String {
    let mut s = d.date.as_str().to_owned();
    if let Some(at) = &d.at {
        s += &format!(" {}", at.time.as_str());
    }
    if let Some(r) = d.repeat {
        s += &format!("，{}", repeat(r));
    }
    s
}

/// `每天`, `每 2 周`, `每 3 个月，从完成那天算`.
pub fn repeat(r: Repeat) -> String {
    let unit = match r.unit {
        RepeatUnit::Day => "天",
        RepeatUnit::Week => "周",
        RepeatUnit::Month => "个月",
    };
    let every = match (r.every.get(), r.unit) {
        (1, RepeatUnit::Month) => "每月".to_owned(),
        (1, _) => format!("每{unit}"),
        (n, _) => format!("每 {n} {unit}"),
    };
    match r.from {
        RepeatFrom::Due => every,
        RepeatFrom::Done => format!("{every}，从完成那天算"),
    }
}

/// `#12 [可做] 出设计稿 · /工作 · 截止 2026-11-20 18:00 · 解锁 1 项`
pub fn task_line(b: &Board, t: &Task, folder: bool, status: bool) -> String {
    let mut line = format!("#{}", t.number);
    if status {
        line += &format!(" {}", status_tag(b.status(t)));
    }
    line += &format!(" {}", t.title.as_str());
    if folder {
        line += &format!(" · {}", b.path(t.folder_id));
    }
    if let Some(d) = &t.due {
        line += &format!(" · 截止 {}", due(d));
    }
    match b.unlocks(t).len() {
        0 => {}
        n => line += &format!(" · 解锁 {n} 项"),
    }
    line
}

/// `#12 [可做] 出设计稿 · /工作`: a task mentioned, by its title shortened.
pub fn mention(b: &Board, t: &Task, folder: bool) -> String {
    let mut line = format!("#{} {} {}", t.number, status_tag(b.status(t)), excerpt(t.title.as_str(), SHORT));
    if folder {
        line += &format!(" · {}", b.path(t.folder_id));
    }
    line
}

/// The data for a task in lists, without the fields that have nothing to say.
pub fn task_view(b: &Board, t: &Task) -> Value {
    let status = b.status(t);
    let mut v = json!({
        "id": t.id, "number": t.number, "title": t.title, "folder": b.path(t.folder_id),
        "status": match status { Status::Ready => "ready", Status::Waiting(_) => "waiting", Status::Done => "done" },
    });
    if let Status::Waiting(n) = status {
        v["waiting_on"] = n.into();
    }
    match b.unlocks(t).len() {
        0 => {}
        n => v["unlocks"] = n.into(),
    }
    if let Some(d) = &t.due {
        v["due"] = json!(d);
    }
    if let Some(at) = t.done_at {
        v["done_at"] = at.into();
    }
    v
}

/// The first line of `s`, cut to `max` characters, with `…` when anything was left out.
pub fn excerpt(s: &str, max: usize) -> String {
    let s = s.trim();
    let first = s.lines().next().unwrap_or_default();
    let short: String = first.chars().take(max).collect();
    if short.len() < s.len() { format!("{short}…") } else { short }
}

/// How long ago, without needing the reader's time zone.
pub fn ago(now: i64, then: i64) -> String {
    match (now - then).max(0) / 1000 {
        s if s < 60 => "刚刚".into(),
        s if s < 3600 => format!("{} 分钟前", s / 60),
        s if s < 86_400 => format!("{} 小时前", s / 3600),
        s => format!("{} 天前", s / 86_400),
    }
}

/// Folder paths and task numbers as they were at one point in the change log.
pub struct Names {
    folders: HashMap<FolderId, (Option<FolderId>, String)>,
    /// Numbers never change, so this keeps every task it has seen.
    tasks: HashMap<TaskId, i64>,
}

impl Names {
    /// The names now.
    pub fn new(snap: &Snapshot) -> Self {
        Names {
            folders: snap.folders.iter().map(|f| (f.id, (f.parent_id, f.name.as_str().to_owned()))).collect(),
            tasks: snap.tasks.iter().map(|t| (t.id, t.number)).collect(),
        }
    }

    /// Steps back over one change, to the names from before it.
    fn undo(&mut self, e: &Effect) {
        match e {
            Effect::Folder(Change::Created(f)) => {
                self.folders.remove(&f.id);
            }
            Effect::Folder(Change::Updated { before: f, .. } | Change::Deleted(f)) => {
                self.folders.insert(f.id, (f.parent_id, f.name.as_str().to_owned()));
            }
            Effect::Task(Change::Created(t) | Change::Updated { after: t, .. } | Change::Deleted(t)) => {
                self.tasks.insert(t.id, t.number);
            }
            Effect::Dependency(_) | Effect::AiJob(_) => {}
        }
    }

    fn path(&self, f: Option<FolderId>) -> String {
        let mut parts = Vec::new();
        let mut cur = f;
        while let Some((parent, name)) = cur.and_then(|id| self.folders.get(&id)) {
            parts.push(name.as_str());
            cur = *parent;
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    /// The path a folder had under `parent` with `name`.
    fn path_in(&self, parent: Option<FolderId>, name: &str) -> String {
        match self.path(parent).as_str() {
            "/" => format!("/{name}"),
            p => format!("{p}/{name}"),
        }
    }

    fn number(&self, id: TaskId) -> String {
        self.tasks.get(&id).map_or_else(|| "已删除的事项".into(), |n| format!("#{n}"))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    FolderCreated,
    FolderRenamed,
    FolderMoved,
    FolderSorted,
    FolderDeleted,
    TaskCreated,
    TaskDone,
    TaskReopened,
    TaskRepeated,
    TaskMoved,
    TaskEdited,
    TaskDeleted,
    DepAdded,
    DepRemoved,
    JobCreated,
    JobStarted,
    JobFailed,
    JobReady,
    JobRetried,
    JobDeleted,
}

impl Kind {
    /// How one change reads, how several read, and what they are counted in.
    fn words(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Kind::FolderCreated => ("新建文件夹", "新建", "个文件夹"),
            Kind::FolderRenamed => ("文件夹改名", "改名", "个文件夹"),
            Kind::FolderMoved => ("移动文件夹", "移动", "个文件夹"),
            Kind::FolderSorted => ("调整文件夹顺序", "调整顺序", "个文件夹"),
            Kind::FolderDeleted => ("删除文件夹", "删除", "个文件夹"),
            Kind::TaskCreated => ("添加", "添加", "项"),
            Kind::TaskDone => ("完成", "完成", "项"),
            Kind::TaskReopened => ("重新打开", "重新打开", "项"),
            Kind::TaskRepeated => ("顺延", "顺延", "项"),
            Kind::TaskMoved => ("移动", "移动", "项"),
            Kind::TaskEdited => ("修改", "修改", "项"),
            Kind::TaskDeleted => ("删除", "删除", "项"),
            Kind::DepAdded => ("添加依赖", "添加", "条依赖"),
            Kind::DepRemoved => ("删除依赖", "删除", "条依赖"),
            Kind::JobCreated => ("提交 AI 整理", "提交 AI 整理", "条"),
            Kind::JobStarted => ("开始 AI 整理", "开始 AI 整理", "条"),
            Kind::JobFailed => ("AI 整理失败", "AI 整理失败", "条"),
            Kind::JobReady => ("AI 整理建议就绪", "AI 整理建议就绪", "条"),
            Kind::JobRetried => ("重试 AI 整理", "重试 AI 整理", "条"),
            Kind::JobDeleted => ("移除 AI 整理请求", "移除 AI 整理请求", "条"),
        }
    }

    /// Its name in data.
    fn code(self) -> &'static str {
        match self {
            Kind::FolderCreated => "folder_created",
            Kind::FolderRenamed => "folder_renamed",
            Kind::FolderMoved => "folder_moved",
            Kind::FolderSorted => "folder_sorted",
            Kind::FolderDeleted => "folder_deleted",
            Kind::TaskCreated => "task_created",
            Kind::TaskDone => "task_done",
            Kind::TaskReopened => "task_reopened",
            Kind::TaskRepeated => "task_repeated",
            Kind::TaskMoved => "task_moved",
            Kind::TaskEdited => "task_edited",
            Kind::TaskDeleted => "task_deleted",
            Kind::DepAdded => "dep_added",
            Kind::DepRemoved => "dep_removed",
            Kind::JobCreated => "job_created",
            Kind::JobStarted => "job_started",
            Kind::JobFailed => "job_failed",
            Kind::JobReady => "job_ready",
            Kind::JobRetried => "job_retried",
            Kind::JobDeleted => "job_deleted",
        }
    }
}

/// `#12 出设计稿`, the title shortened.
pub fn item(t: &Task) -> String {
    format!("#{} {}", t.number, excerpt(t.title.as_str(), SHORT))
}

/// An AI job by the start of its text.
pub fn job_item(j: &AiJob) -> String {
    format!("“{}”", excerpt(j.text.as_str(), 20))
}

/// The due date a repeating task moved on to, done: only its date changed, and to a later one.
fn moved_on<'a>(before: &Task, after: &'a Task) -> Option<&'a Date> {
    match (&before.due, &after.due) {
        (Some(b), Some(a)) if b.repeat.is_some() && a.repeat == b.repeat && a.at == b.at && a.date > b.date => Some(&a.date),
        _ => None,
    }
}

/// What changed in a task besides whether it is done, which folder it is in, and a repeating one's
/// due date moving on; its place in a folder only within the same folder, as a move puts it
/// somewhere in the new one anyway.
fn edited(before: &Task, after: &Task) -> Vec<&'static str> {
    [
        ("title", before.title != after.title),
        ("description", before.description != after.description),
        ("due", before.due != after.due && moved_on(before, after).is_none()),
        ("order", before.sort_key != after.sort_key && before.folder_id == after.folder_id),
    ]
    .into_iter()
    .filter_map(|(field, changed)| changed.then_some(field))
    .collect()
}

/// Each kind of change one change amounts to, as it reads and as data, given the names right after
/// it: renaming and moving a folder at once is two, and so is completing a task while retitling it.
/// A change touches one entity, so its old and new places both read in that tree.
fn describe(e: &Effect, n: &Names) -> Vec<(Kind, String, Value)> {
    match e {
        Effect::Folder(Change::Created(f)) => {
            let path = n.path_in(f.parent_id, f.name.as_str());
            vec![(Kind::FolderCreated, path.clone(), json!({ "folder": path }))]
        }
        Effect::Folder(Change::Updated { before, after }) => {
            let old = n.path_in(before.parent_id, before.name.as_str());
            let mut found = Vec::new();
            if before.name != after.name {
                found.push((Kind::FolderRenamed, format!("{old} → {}", after.name.as_str()), json!({ "folder": old, "name": after.name })));
            }
            if before.parent_id != after.parent_id {
                let to = n.path_in(after.parent_id, after.name.as_str());
                found.push((Kind::FolderMoved, format!("{old} → {to}"), json!({ "folder": old, "to": to })));
            }
            // Its place among the same folders; a move puts it somewhere in the new one anyway.
            if before.parent_id == after.parent_id && before.sort_key != after.sort_key {
                found.push((Kind::FolderSorted, old.clone(), json!({ "folder": old })));
            }
            found
        }
        Effect::Folder(Change::Deleted(f)) => {
            let path = n.path_in(f.parent_id, f.name.as_str());
            vec![(Kind::FolderDeleted, path.clone(), json!({ "folder": path }))]
        }
        Effect::Task(Change::Created(t)) => vec![(Kind::TaskCreated, item(t), json!({ "task": t.number }))],
        Effect::Task(Change::Updated { before, after: t }) => {
            let mut found = Vec::new();
            match (before.done_at, t.done_at) {
                (None, Some(_)) => found.push((Kind::TaskDone, item(t), json!({ "task": t.number }))),
                (Some(_), None) => found.push((Kind::TaskReopened, item(t), json!({ "task": t.number }))),
                _ => {}
            }
            if let Some(date) = moved_on(before, t) {
                found.push((Kind::TaskRepeated, format!("{} → {}", item(t), date.as_str()), json!({ "task": t.number, "due": date })));
            }
            if before.folder_id != t.folder_id {
                let to = n.path(t.folder_id);
                found.push((Kind::TaskMoved, format!("{} → {to}", item(t)), json!({ "task": t.number, "to": to })));
            }
            let fields = edited(before, t);
            if !fields.is_empty() {
                found.push((Kind::TaskEdited, item(t), json!({ "task": t.number, "fields": fields })));
            }
            found
        }
        Effect::Task(Change::Deleted(t)) => vec![(Kind::TaskDeleted, item(t), json!({ "task": t.number }))],
        Effect::Dependency(link @ (Link::Added(d) | Link::Removed(d))) => {
            let kind = if matches!(link, Link::Added(_)) { Kind::DepAdded } else { Kind::DepRemoved };
            let text = format!("{} → {}", n.number(d.before), n.number(d.after));
            vec![(kind, text, json!({ "before": n.tasks.get(&d.before), "after": n.tasks.get(&d.after) }))]
        }
        Effect::AiJob(Change::Created(j)) => vec![(Kind::JobCreated, job_item(j), json!({ "job": j.id }))],
        Effect::AiJob(Change::Updated { after: j, .. }) => vec![match j.state {
            JobState::Running => (Kind::JobStarted, job_item(j), json!({ "job": j.id })),
            JobState::Failed => {
                let error = j.error.as_deref().unwrap_or_default();
                (Kind::JobFailed, format!("{}（{error}）", job_item(j)), json!({ "job": j.id, "error": error }))
            }
            JobState::Waiting => (Kind::JobRetried, job_item(j), json!({ "job": j.id })),
            JobState::Ready => (Kind::JobReady, job_item(j), json!({ "job": j.id })),
        }],
        Effect::AiJob(Change::Deleted(j)) => vec![(Kind::JobDeleted, job_item(j), json!({ "job": j.id }))],
    }
}

/// The task to list a change by.
fn number(e: &Effect, n: &Names) -> Option<i64> {
    match e {
        Effect::Folder(_) | Effect::AiJob(_) => None,
        Effect::Task(Change::Created(t) | Change::Updated { after: t, .. } | Change::Deleted(t)) => Some(t.number),
        Effect::Dependency(Link::Added(d) | Link::Removed(d)) => n.tasks.get(&d.before).copied(),
    }
}

/// What a batch changed: one line per kind of change, listing at most `limit` items per line, and
/// each change as data, in order. `names` are those from after the batch; this steps them back over
/// it, one change at a time, so that every change reads with the folders as they were right after
/// it, also in the middle of a batch and going through the log newest first.
pub fn changes(effects: &[Effect], names: &mut Names, limit: usize) -> (Vec<String>, Vec<Value>) {
    let mut described = Vec::with_capacity(effects.len());
    for e in effects.iter().rev() {
        described.push((number(e, names), describe(e, names)));
        names.undo(e);
    }
    described.reverse();
    let mut groups: BTreeMap<Kind, Vec<(Option<i64>, String)>> = BTreeMap::new();
    let mut data = Vec::with_capacity(described.len());
    for (number, found) in described {
        for (kind, item, mut d) in found {
            groups.entry(kind).or_default().push((number, item));
            d["kind"] = kind.code().into();
            data.push(d);
        }
    }
    let lines = groups
        .into_iter()
        .map(|(kind, mut items)| {
            items.sort_by_key(|(number, _)| *number);
            let items: Vec<String> = items.into_iter().map(|(_, item)| item).collect();
            let (one, many, unit) = kind.words();
            match items.as_slice() {
                [only] => format!("{one} {only}"),
                _ => {
                    let more = if items.len() > limit { " 等" } else { "" };
                    format!("{many} {} {unit}：{}{more}", items.len(), items[..items.len().min(limit)].join("、"))
                }
            }
        })
        .collect();
    (lines, data)
}
