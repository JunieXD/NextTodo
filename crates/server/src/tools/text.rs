//! Text output: short Chinese lines meant for people and agents alike. See `docs/agent.md`.

use std::collections::{BTreeMap, HashMap};

use nexttodo_core::board::{Board, Snapshot, Status};
use nexttodo_core::model::{Change, Due, Effect, FolderId, Link, Task, TaskId};
use serde_json::{Value, json};

pub fn status_tag(s: Status) -> String {
    match s {
        Status::Ready => "[可做]".into(),
        Status::Waiting(n) => format!("[等 {n} 项]"),
        Status::Done => "[已完成]".into(),
    }
}

pub fn due(d: &Due) -> String {
    match &d.at {
        Some(at) => format!("{} {}", d.date.as_str(), at.time.as_str()),
        None => d.date.as_str().to_owned(),
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

/// The data for a task in lists.
pub fn task_view(b: &Board, t: &Task) -> Value {
    let (status, waiting_on) = match b.status(t) {
        Status::Ready => ("ready", 0),
        Status::Waiting(n) => ("waiting", n),
        Status::Done => ("done", 0),
    };
    json!({
        "id": t.id, "number": t.number, "title": t.title, "status": status, "waiting_on": waiting_on,
        "unlocks": b.unlocks(t).len(), "folder": b.path(t.folder_id), "due": t.due, "done_at": t.done_at,
    })
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
#[derive(Clone)]
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

    /// Steps back over a batch, to the names from before it.
    fn undo(&mut self, effects: &[Effect]) {
        for e in effects.iter().rev() {
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
                Effect::Dependency(_) => {}
            }
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
    TaskMoved,
    TaskEdited,
    TaskDeleted,
    DepAdded,
    DepRemoved,
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
            Kind::TaskMoved => ("移动", "移动", "项"),
            Kind::TaskEdited => ("修改", "修改", "项"),
            Kind::TaskDeleted => ("删除", "删除", "项"),
            Kind::DepAdded => ("添加依赖", "添加", "条依赖"),
            Kind::DepRemoved => ("删除依赖", "删除", "条依赖"),
        }
    }
}

fn item(t: &Task) -> String {
    format!("#{} {}", t.number, t.title.as_str())
}

/// The kind of change, a task number to list it by, and how it reads, given the names from before
/// and after its batch.
fn describe(e: &Effect, n: &Names, then: &Names) -> (Kind, Option<i64>, String) {
    let (kind, item) = match e {
        Effect::Folder(Change::Created(f)) => (Kind::FolderCreated, then.path(Some(f.id))),
        Effect::Folder(Change::Updated { before, after }) => {
            let old = n.path_in(before.parent_id, before.name.as_str());
            if before.name != after.name {
                (Kind::FolderRenamed, format!("{old} → {}", after.name.as_str()))
            } else if before.parent_id != after.parent_id {
                (Kind::FolderMoved, format!("{old} → {}", then.path(Some(after.id))))
            } else {
                (Kind::FolderSorted, old)
            }
        }
        Effect::Folder(Change::Deleted(f)) => (Kind::FolderDeleted, n.path_in(f.parent_id, f.name.as_str())),
        Effect::Task(Change::Created(t)) => (Kind::TaskCreated, item(t)),
        Effect::Task(Change::Updated { before, after }) => match (before.done_at, after.done_at) {
            (None, Some(_)) => (Kind::TaskDone, item(after)),
            (Some(_), None) => (Kind::TaskReopened, item(after)),
            _ if before.folder_id != after.folder_id => (Kind::TaskMoved, format!("{} → {}", item(after), then.path(after.folder_id))),
            _ => (Kind::TaskEdited, item(after)),
        },
        Effect::Task(Change::Deleted(t)) => (Kind::TaskDeleted, item(t)),
        Effect::Dependency(Link::Added(d)) => (Kind::DepAdded, format!("{} → {}", n.number(d.before), n.number(d.after))),
        Effect::Dependency(Link::Removed(d)) => (Kind::DepRemoved, format!("{} → {}", n.number(d.before), n.number(d.after))),
    };
    let number = match e {
        Effect::Folder(_) => None,
        Effect::Task(Change::Created(t) | Change::Updated { after: t, .. } | Change::Deleted(t)) => Some(t.number),
        Effect::Dependency(Link::Added(d) | Link::Removed(d)) => n.tasks.get(&d.before).copied(),
    };
    (kind, number, item)
}

/// What a batch changed, one line per kind of change, listing at most `limit` items per line.
/// `names` are those from after the batch; this steps them back to before it, so that going through
/// the log newest first, each batch reads with the folders as they were at the time.
pub fn changes(effects: &[Effect], names: &mut Names, limit: usize) -> Vec<String> {
    let then = names.clone();
    names.undo(effects);
    let mut groups: BTreeMap<Kind, Vec<(Option<i64>, String)>> = BTreeMap::new();
    for e in effects {
        let (kind, number, item) = describe(e, names, &then);
        groups.entry(kind).or_default().push((number, item));
    }
    groups
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
        .collect()
}
