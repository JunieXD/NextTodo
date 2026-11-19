//! Text output: short Chinese lines meant for people and agents alike. See `docs/agent.md`.

use std::collections::{BTreeMap, HashMap};

use nexttodo_core::board::{Board, Snapshot, Status};
use nexttodo_core::model::{
    AiJob, Change, Date, Due, Effect, FolderId, Holder, JobState, Link, Period, Person, Repeat, RepeatFrom, Task, TaskId, Time, Weekdays,
};
use serde_json::{Value, json};

/// How much of a title is shown where a task is only mentioned, not listed.
const SHORT: usize = 30;

/// `[可做]`, `[等 2 项]`, `[2026-11-14 09:30 开始]`, both `[等 2 项，2026-11-14 开始]`, or `[已完成]`.
pub fn status_tag(b: &Board, t: &Task) -> String {
    let start = || format!("{} 开始", start(t).expect("a start to wait for"));
    match b.status(t) {
        Status::Ready => "[可做]".into(),
        Status::Waiting { on, not_started: false } => format!("[等 {on} 项]"),
        Status::Waiting { on: 0, not_started: true } => format!("[{}]", start()),
        Status::Waiting { on, not_started: true } => format!("[等 {on} 项，{}]", start()),
        Status::Done => "[已完成]".into(),
    }
}

/// `2026-11-20 18:00`, with how it repeats: `2026-11-20，每周`, `2026-11-20，每天，到 2026-12-19`.
/// When it starts, `2026-11-14` or `2026-11-14 09:30`, if it has a start.
pub fn start(t: &Task) -> Option<String> {
    Some(moment(t.start.as_ref()?, t.start_time.as_ref()))
}

/// `2026-11-14`, or `2026-11-14 09:30` with a time.
pub fn moment(date: &Date, time: Option<&Time>) -> String {
    time.map_or_else(|| date.as_str().to_owned(), |time| format!("{} {}", date.as_str(), time.as_str()))
}

pub fn due(d: &Due) -> String {
    let mut s = d.date.as_str().to_owned();
    if let Some(at) = &d.at {
        s += &format!(" {}", at.time.as_str());
    }
    if let Some(r) = &d.repeat {
        s += &format!("，{}", repeat(r));
    }
    s
}

/// `每天`, `每 2 周`, `每个工作日`, `每 2 周的周二、四`, `每月 1、15 号`, `每年`, `每 3 个月，从完成那天算`,
/// `每天，到 2026-12-19`; the web app's `repeatLabel` has them so too, but for the last day.
pub fn repeat(r: &Repeat) -> String {
    let every = match (r.every.get(), r.period) {
        (1, Period::Week(Some(Weekdays::WORKDAYS))) => "每个工作日".to_owned(),
        (n, Period::Week(Some(days))) => {
            let names: Vec<&str> =
                (0..7).filter(|&i| days.has(i)).map(|i| ["一", "二", "三", "四", "五", "六", "日"][i as usize]).collect();
            let names = names.join("、");
            if n == 1 { format!("每周{names}") } else { format!("每 {n} 周的周{names}") }
        }
        (n, Period::Month(Some(days))) => {
            let days: Vec<String> = Vec::from(days).iter().map(u8::to_string).collect();
            let days = days.join("、");
            if n == 1 { format!("每月 {days} 号") } else { format!("每 {n} 个月的 {days} 号") }
        }
        (12, Period::Month(None)) => "每年".to_owned(),
        (n, Period::Month(None)) if n % 12 == 0 => format!("每 {} 年", n / 12),
        (1, Period::Day) => "每天".to_owned(),
        (1, Period::Week(None)) => "每周".to_owned(),
        (1, Period::Month(None)) => "每月".to_owned(),
        (n, Period::Day) => format!("每 {n} 天"),
        (n, Period::Week(None)) => format!("每 {n} 周"),
        (n, Period::Month(None)) => format!("每 {n} 个月"),
    };
    let every = match r.from {
        RepeatFrom::Due => every,
        RepeatFrom::Done => format!("{every}，从完成那天算"),
    };
    match &r.until {
        Some(until) => format!("{every}，到 {}", until.as_str()),
        None => every,
    }
}

/// `#12 [可做] 出设计稿 · /工作 · 负责 bob · 截止 2026-11-20 18:00 · 第 1/3 步 · 解锁 1 项`, with the
/// step where it is given.
pub fn task_line(b: &Board, t: &Task, folder: bool, status: bool, step: Option<(usize, usize)>) -> String {
    let mut line = b.label(t);
    if status {
        line += &format!(" {}", status_tag(b, t));
    }
    line += &format!(" {}", t.title.as_str());
    if folder {
        line += &format!(" · {}", b.path(t.folder_id));
    }
    if !t.assignees.is_empty() {
        line += &format!(" · 负责 {}", usernames(b, &t.assignees).join("、"));
    }
    if let Some(d) = &t.due {
        line += &format!(" · 截止 {}", due(d));
    }
    if let Some((n, of)) = step {
        line += &format!(" · 第 {n}/{of} 步");
    }
    match b.unlocks(t).len() {
        0 => {}
        n => line += &format!(" · 解锁 {n} 项"),
    }
    line
}

/// The usernames of people by user id.
pub fn usernames<'a>(b: &Board<'a>, ids: &'a [String]) -> Vec<&'a str> {
    ids.iter().map(|id| b.person(id).map_or(id.as_str(), |p| p.username.as_str())).collect()
}

/// `#12 [可做] 出设计稿 · /工作`: a task mentioned, by its title shortened.
pub fn mention(b: &Board, t: &Task, folder: bool) -> String {
    let mut line = format!("{} {} {}", b.label(t), status_tag(b, t), excerpt(t.title.as_str(), SHORT));
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
        "status": match status { Status::Ready => "ready", Status::Waiting { .. } => "waiting", Status::Done => "done" },
    });
    if let Some(owner) = foreign(b, t) {
        v["owner"] = owner.into();
    }
    if !t.assignees.is_empty() {
        v["assignees"] = usernames(b, &t.assignees).into();
    }
    if let Status::Waiting { on, .. } = status
        && on > 0
    {
        v["waiting_on"] = on.into();
    }
    match b.unlocks(t).len() {
        0 => {}
        n => v["unlocks"] = n.into(),
    }
    if let Some(s) = &t.start {
        v["start"] = json!(s);
    }
    if let Some(time) = &t.start_time {
        v["start_time"] = json!(time);
    }
    if let Some(d) = &t.due {
        v["due"] = json!(d);
    }
    if let Some(at) = t.done_at {
        v["done_at"] = at.into();
    }
    if let Some(c) = &t.completions {
        v["completions"] = json!(c);
    }
    v
}

/// The username of whose a task is, when it is someone else's.
fn foreign<'a>(b: &Board<'a>, t: &Task) -> Option<&'a str> {
    let owner = b.owner(t);
    (owner != b.me()).then(|| b.person(owner).map_or(owner, |p| p.username.as_str()))
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

/// Folder paths, task numbers and people as they were at one point in the change log, for the user
/// `me`.
pub struct Names {
    me: String,
    people: HashMap<String, Person>,
    folders: HashMap<FolderId, (Option<FolderId>, String)>,
    /// The shared folders, by whose they are.
    shares: HashMap<FolderId, String>,
    /// A task's number changes only as it moves into or out of a folder someone else shares, so
    /// this keeps every task it has seen, by its latest number and whose it was then.
    tasks: HashMap<TaskId, (i64, Option<String>)>,
}

impl Names {
    /// The names now, in `snap`, also of the `others` the changes to read mention whom it doesn't
    /// know (`strangers`).
    pub fn new(snap: &Snapshot, me: &str, others: Vec<Person>) -> Self {
        let mut names = Names {
            me: me.to_owned(),
            people: snap.people.iter().cloned().chain(others).map(|p| (p.id.clone(), p)).collect(),
            folders: snap.folders.iter().map(|f| (f.id, (f.parent_id, f.name.as_str().to_owned()))).collect(),
            shares: snap.shares.iter().map(|sh| (sh.folder_id, sh.owner.clone())).collect(),
            tasks: HashMap::new(),
        };
        for t in &snap.tasks {
            names.seen(t);
        }
        names
    }

    fn seen(&mut self, t: &Task) {
        let owner = self.foreign(t.folder_id);
        self.tasks.insert(t.id, (t.number, owner));
    }

    /// The username of whose what is in folder `f` is, when that is someone else.
    fn foreign(&self, f: Option<FolderId>) -> Option<String> {
        let mut cur = f;
        while let Some(id) = cur {
            if let Some(owner) = self.shares.get(&id) {
                return (*owner != self.me).then(|| self.username(owner));
            }
            cur = self.folders.get(&id).and_then(|(parent, _)| *parent);
        }
        None
    }

    pub fn username(&self, id: &str) -> String {
        self.people.get(id).map_or_else(|| id.to_owned(), |p| p.username.clone())
    }

    /// Someone's display name.
    pub fn person(&self, id: &str) -> String {
        self.people.get(id).map_or_else(|| id.to_owned(), |p| p.display_name.clone())
    }

    /// A task's number in data, with whose it is when it is someone else's.
    fn task(&self, t: &Task) -> Value {
        match self.foreign(t.folder_id) {
            Some(owner) => json!({ "task": t.number, "owner": owner }),
            None => json!({ "task": t.number }),
        }
    }

    /// `#12 出设计稿`, or `@alice#12 出设计稿`.
    fn item(&self, t: &Task) -> String {
        let label = match self.foreign(t.folder_id) {
            Some(owner) => format!("@{owner}#{}", t.number),
            None => format!("#{}", t.number),
        };
        format!("{label} {}", excerpt(t.title.as_str(), SHORT))
    }

    /// Steps back over one change, to the names from before it. Sharing is stepped over a change at a
    /// time, by `changes`.
    fn undo(&mut self, e: &Effect) {
        match e {
            Effect::Folder(Change::Created(f)) => {
                self.folders.remove(&f.id);
            }
            Effect::Folder(Change::Updated { before: f, .. } | Change::Deleted(f)) => {
                self.folders.insert(f.id, (f.parent_id, f.name.as_str().to_owned()));
            }
            Effect::Task(Change::Created(t) | Change::Deleted(t)) | Effect::Task(Change::Updated { before: t, .. }) => self.seen(t),
            Effect::Dependency(_) | Effect::Attachment(_) | Effect::AiJob(_) | Effect::Share(_) | Effect::Person(_) => {}
        }
    }

    fn path(&self, f: Option<FolderId>) -> String {
        match f.and_then(|id| self.folders.get(&id)) {
            Some((parent, name)) => self.path_of(f.expect("a folder"), *parent, name),
            None => "/".into(),
        }
    }

    /// The path folder `id` had under `parent` with `name`: `/工作/网站改版`, or `@alice/装修/水电`.
    fn path_of(&self, id: FolderId, parent: Option<FolderId>, name: &str) -> String {
        let mut parts = vec![name];
        let mut cur = parent;
        while let Some((up, name)) = cur.and_then(|id| self.folders.get(&id)) {
            parts.push(name.as_str());
            cur = *up;
        }
        parts.reverse();
        let owner = if self.shares.contains_key(&id) { self.foreign(Some(id)) } else { self.foreign(parent) };
        match owner {
            Some(owner) => format!("@{owner}/{}", parts.join("/")),
            None => format!("/{}", parts.join("/")),
        }
    }

    /// What an attachment belongs to: `#12`, or the AI job it came with.
    fn holder(&self, h: Holder) -> (String, Value) {
        match h {
            Holder::Task(t) => (self.number(t), self.tasks.get(&t).map_or(Value::Null, |(n, _)| (*n).into())),
            Holder::Job(_) => ("AI 整理请求".into(), Value::Null),
        }
    }

    fn number(&self, id: TaskId) -> String {
        match self.tasks.get(&id) {
            None => "已删除的事项".into(),
            Some((n, None)) => format!("#{n}"),
            Some((n, Some(owner))) => format!("@{owner}#{n}"),
        }
    }
}

/// The people `effects` mention whom `snap` doesn't know, as when they no longer share a folder
/// with the user: `Names` needs them too.
pub fn strangers<'a>(snap: &Snapshot, effects: impl IntoIterator<Item = &'a Effect>) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for e in effects {
        let shares = match e {
            Effect::Share(Change::Created(sh) | Change::Deleted(sh)) => vec![sh],
            Effect::Share(Change::Updated { before, after }) => vec![before, after],
            _ => continue,
        };
        for id in shares.into_iter().flat_map(|sh| std::iter::once(&sh.owner).chain(&sh.members)) {
            if !snap.people.iter().any(|p| p.id == *id) && !found.contains(id) {
                found.push(id.clone());
            }
        }
    }
    found
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
    AttachmentAdded,
    AttachmentRenamed,
    AttachmentMoved,
    AttachmentDeleted,
    JobCreated,
    JobStarted,
    JobFailed,
    JobReady,
    JobRetried,
    JobDeleted,
    MemberAdded,
    MemberRemoved,
    PersonRenamed,
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
            Kind::AttachmentAdded => ("添加附件", "添加", "个附件"),
            Kind::AttachmentRenamed => ("附件改名", "改名", "个附件"),
            Kind::AttachmentMoved => ("移动附件", "移动", "个附件"),
            Kind::AttachmentDeleted => ("删除附件", "删除", "个附件"),
            Kind::JobCreated => ("提交 AI 整理", "提交 AI 整理", "条"),
            Kind::JobStarted => ("开始 AI 整理", "开始 AI 整理", "条"),
            Kind::JobFailed => ("AI 整理失败", "AI 整理失败", "条"),
            Kind::JobReady => ("AI 整理建议就绪", "AI 整理建议就绪", "条"),
            Kind::JobRetried => ("重试 AI 整理", "重试 AI 整理", "条"),
            Kind::JobDeleted => ("移除 AI 整理请求", "移除 AI 整理请求", "条"),
            Kind::MemberAdded => ("添加成员", "添加成员", "人"),
            Kind::MemberRemoved => ("移除成员", "移除成员", "人"),
            Kind::PersonRenamed => ("用户改名", "改名", "人"),
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
            Kind::AttachmentAdded => "attachment_added",
            Kind::AttachmentRenamed => "attachment_renamed",
            Kind::AttachmentMoved => "attachment_moved",
            Kind::AttachmentDeleted => "attachment_deleted",
            Kind::JobCreated => "job_created",
            Kind::JobStarted => "job_started",
            Kind::JobFailed => "job_failed",
            Kind::JobReady => "job_ready",
            Kind::JobRetried => "job_retried",
            Kind::JobDeleted => "job_deleted",
            Kind::MemberAdded => "member_added",
            Kind::MemberRemoved => "member_removed",
            Kind::PersonRenamed => "person_renamed",
        }
    }
}

/// A file's size as people read it: `820 B`, `320 KB`, `1.2 MB`.
pub fn size(bytes: u32) -> String {
    const KB: u32 = 1024;
    const MB: u32 = KB * KB;
    match bytes {
        0..KB => format!("{bytes} B"),
        KB..MB => format!("{} KB", bytes.div_ceil(KB)),
        _ => format!("{:.1} MB", f64::from(bytes) / f64::from(MB)).replace(".0 MB", " MB"),
    }
}

/// `#12 出设计稿`, the title shortened.
pub fn item(b: &Board, t: &Task) -> String {
    format!("{} {}", b.label(t), excerpt(t.title.as_str(), SHORT))
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

/// Whether a repeating task was done for good, the repeat ending with it: the last time, or ended.
fn ended(before: &Task, after: &Task) -> bool {
    match (&before.due, &after.due) {
        (Some(b), Some(a)) => after.done_at.is_some() && b.repeat.is_some() && *a == Due { repeat: None, ..b.clone() },
        _ => false,
    }
}

/// What changed in a task besides whether it is done, which folder it is in, and a repeating one's
/// due date moving on or its repeat ending as it is done; its place in a folder only within the same
/// folder, as a move puts it somewhere in the new one anyway.
fn edited(before: &Task, after: &Task) -> Vec<&'static str> {
    [
        ("title", before.title != after.title),
        ("description", before.description != after.description),
        ("due", before.due != after.due && moved_on(before, after).is_none() && !ended(before, after)),
        ("assignees", before.assignees != after.assignees),
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
            let path = n.path_of(f.id, f.parent_id, f.name.as_str());
            vec![(Kind::FolderCreated, path.clone(), json!({ "folder": path }))]
        }
        Effect::Folder(Change::Updated { before, after }) => {
            let old = n.path_of(before.id, before.parent_id, before.name.as_str());
            let mut found = Vec::new();
            if before.name != after.name {
                found.push((Kind::FolderRenamed, format!("{old} → {}", after.name.as_str()), json!({ "folder": old, "name": after.name })));
            }
            if before.parent_id != after.parent_id {
                let to = n.path_of(after.id, after.parent_id, after.name.as_str());
                found.push((Kind::FolderMoved, format!("{old} → {to}"), json!({ "folder": old, "to": to })));
            }
            // Its place among the same folders; a move puts it somewhere in the new one anyway.
            if before.parent_id == after.parent_id && before.sort_key != after.sort_key {
                found.push((Kind::FolderSorted, old.clone(), json!({ "folder": old })));
            }
            found
        }
        Effect::Folder(Change::Deleted(f)) => {
            let path = n.path_of(f.id, f.parent_id, f.name.as_str());
            vec![(Kind::FolderDeleted, path.clone(), json!({ "folder": path }))]
        }
        Effect::Task(Change::Created(t)) => vec![(Kind::TaskCreated, n.item(t), n.task(t))],
        Effect::Task(Change::Updated { before, after: t }) => {
            let mut found = Vec::new();
            let with = |mut v: Value, key: &str, value: Value| {
                v[key] = value;
                v
            };
            match (before.done_at, t.done_at) {
                (None, Some(_)) => found.push((Kind::TaskDone, n.item(t), n.task(t))),
                (Some(_), None) => found.push((Kind::TaskReopened, n.item(t), n.task(t))),
                _ => {}
            }
            if let Some(date) = moved_on(before, t) {
                found.push((Kind::TaskRepeated, format!("{} → {}", n.item(t), date.as_str()), with(n.task(t), "due", json!(date))));
            }
            if before.folder_id != t.folder_id {
                let to = n.path(t.folder_id);
                let mut v = with(n.task(before), "to", json!(to));
                if t.number != before.number {
                    v["number"] = t.number.into();
                }
                found.push((Kind::TaskMoved, format!("{} → {to}", n.item(before)), v));
            }
            let fields = edited(before, t);
            if !fields.is_empty() {
                found.push((Kind::TaskEdited, n.item(t), with(n.task(t), "fields", json!(fields))));
            }
            found
        }
        Effect::Task(Change::Deleted(t)) => vec![(Kind::TaskDeleted, n.item(t), n.task(t))],
        Effect::Dependency(link @ (Link::Added(d) | Link::Removed(d))) => {
            let kind = if matches!(link, Link::Added(_)) { Kind::DepAdded } else { Kind::DepRemoved };
            let text = format!("{} → {}", n.number(d.before), n.number(d.after));
            let number = |id| n.tasks.get(&id).map(|(number, _)| *number);
            let mut v = json!({ "before": number(d.before), "after": number(d.after) });
            if let Some((_, Some(owner))) = n.tasks.get(&d.before) {
                v["owner"] = owner.as_str().into();
            }
            vec![(kind, text, v)]
        }
        Effect::Attachment(Change::Created(a) | Change::Deleted(a)) => {
            let kind = if matches!(e, Effect::Attachment(Change::Created(_))) { Kind::AttachmentAdded } else { Kind::AttachmentDeleted };
            let (of, task) = n.holder(a.holder);
            vec![(kind, format!("{}（{of}）", a.name.as_str()), json!({ "attachment": a.id, "name": a.name, "task": task }))]
        }
        Effect::Attachment(Change::Updated { before, after }) => {
            let mut found = Vec::new();
            if before.name != after.name {
                let text = format!("{} → {}", before.name.as_str(), after.name.as_str());
                found.push((Kind::AttachmentRenamed, text, json!({ "attachment": after.id, "name": after.name })));
            }
            if before.holder != after.holder {
                let (to, task) = n.holder(after.holder);
                found.push((
                    Kind::AttachmentMoved,
                    format!("{} → {to}", after.name.as_str()),
                    json!({ "attachment": after.id, "task": task }),
                ));
            }
            found
        }
        Effect::Share(change) => {
            let (folder, before, after) = match change {
                Change::Created(a) => (a.folder_id, &[][..], &a.members[..]),
                Change::Updated { before, after } => (before.folder_id, &before.members[..], &after.members[..]),
                Change::Deleted(b) => (b.folder_id, &b.members[..], &[][..]),
            };
            let path = n.path(Some(folder));
            let mut found = Vec::new();
            for (kind, people, other) in [(Kind::MemberAdded, after, before), (Kind::MemberRemoved, before, after)] {
                for u in people.iter().filter(|u| !other.contains(u)) {
                    found.push((kind, format!("{}（{path}）", n.person(u)), json!({ "folder": path, "user": n.username(u) })));
                }
            }
            found
        }
        Effect::Person(Change::Updated { before, after }) => vec![(
            Kind::PersonRenamed,
            format!("{} → {}", before.display_name, after.display_name),
            json!({ "user": after.username, "name": after.display_name }),
        )],
        // Someone met in a folder shared since: what is new is the sharing.
        Effect::Person(_) => Vec::new(),
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
        Effect::Folder(_) | Effect::AiJob(_) | Effect::Share(_) | Effect::Person(_) => None,
        Effect::Task(Change::Created(t) | Change::Updated { after: t, .. } | Change::Deleted(t)) => Some(t.number),
        Effect::Dependency(Link::Added(d) | Link::Removed(d)) => n.tasks.get(&d.before).map(|(number, _)| *number),
        Effect::Attachment(Change::Created(a) | Change::Updated { after: a, .. } | Change::Deleted(a)) => match a.holder {
            Holder::Task(t) => n.tasks.get(&t).map(|(number, _)| *number),
            Holder::Job(_) => None,
        },
    }
}

/// What a batch changed: one line per kind of change, listing at most `limit` items per line, and
/// each change as data, in order. `names` are those from after the batch; this steps them back over
/// it, one change at a time, so that every change reads with the folders as they were right after
/// it, also in the middle of a batch and going through the log newest first.
pub fn changes(effects: &[Effect], names: &mut Names, limit: usize) -> (Vec<String>, Vec<Value>) {
    // Sharing changes nothing of whose what is: what a change that stops sharing a folder did in it
    // is its owner's, as before it.
    for e in effects {
        if let Effect::Share(Change::Created(sh) | Change::Deleted(sh) | Change::Updated { before: sh, .. }) = e {
            names.shares.insert(sh.folder_id, sh.owner.clone());
        }
    }
    let mut described = Vec::with_capacity(effects.len());
    for e in effects.iter().rev() {
        described.push((number(e, names), describe(e, names)));
        names.undo(e);
    }
    for e in effects {
        if let Effect::Share(Change::Created(sh)) = e {
            names.shares.remove(&sh.folder_id);
        }
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
