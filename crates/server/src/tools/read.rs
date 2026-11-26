//! Tools that only read.

use std::collections::{HashMap, HashSet};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use nexttodo_core::board::{Board, Status};
use nexttodo_core::calendar;
use nexttodo_core::model::{Attachment, Completion, Effect, FolderId, Holder, JobState, Share, Task, TaskId};
use nexttodo_core::ops::Op;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use ts_rs::TS;

use super::args::{AgendaArgs, AttachmentArgs, GraphArgs, ListArgs, LogArgs, NextArgs, NoArgs, SearchArgs, ShowArgs};
use super::text::{self, LogItem, Names, ago, job_item, mention, status_tag, task_line, task_view, usernames};
use super::{Cx, Output, find_folder, find_task};
use crate::attachments::showable_image;
use crate::error::ApiError;
use crate::store;

/// The largest text file `attachment` gives the content of.
const MAX_TEXT: u32 = 1024 * 1024;

/// How many tasks a list shows, unless told otherwise.
const LIMIT: u32 = 50;

/// How many tasks a list shows: `limit`, or 50.
fn limit(limit: Option<u32>) -> Result<usize, ApiError> {
    match limit.unwrap_or(LIMIT) {
        n @ 1..=500 => Ok(n as usize),
        _ => Err(ApiError::invalid("limit 要在 1 到 500 之间", Some("limit".into()))),
    }
}

/// Task lines, and the last one saying how many more there are when `total` is more than shown.
fn listed(mut lines: Vec<String>, shown: usize, total: usize) -> Vec<String> {
    if total > shown {
        lines.push(format!("另有 {} 项未列出，可调大 limit", total - shown));
    }
    lines
}

/// `（待办 3，可做 1）` for the tasks in a folder and its subfolders, and the same as data.
fn counts(b: &Board, f: Option<FolderId>) -> (String, usize, usize) {
    let tasks = b.within(f);
    let pending = tasks.iter().filter(|t| t.done_at.is_none()).count();
    let ready = tasks.iter().filter(|t| b.status(t) == Status::Ready).count();
    let text = if pending == 0 { "（无待办）".to_owned() } else { format!("（待办 {pending}，可做 {ready}）") };
    (text, pending, ready)
}

/// Who shares a folder with whom, in words and data.
fn shared(b: &Board, share: &Share) -> (String, Value) {
    let owner = usernames(b, std::slice::from_ref(&share.owner))[0];
    let members = usernames(b, &share.members);
    (format!("共享人 {owner}，成员 {}", members.join("、")), json!({ "owner": owner, "members": members }))
}

pub fn tree(c: &Connection, cx: &Cx, _: NoArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let (mut lines, mut data) = (Vec::new(), Vec::new());
    for f in b.folder_tree() {
        let folder = f.and_then(|id| b.folder(id));
        let name = folder.map_or("/", |f| f.name.as_str());
        let depth = std::iter::successors(f, |id| b.folder(*id).and_then(|f| f.parent_id)).count();
        let (text, pending, ready) = counts(&b, f);
        let mut v = json!({ "path": b.path(f), "name": name, "depth": depth, "pending": pending, "ready": ready });
        if let Some(color) = folder.and_then(|f| f.color) {
            v["color"] = json!(color);
        }
        if let Some(icon) = folder.and_then(|f| f.icon) {
            v["icon"] = json!(icon);
        }
        // Those shared with the user go by their path, which says whose they are.
        let foreign = b.shared_with_me().iter().any(|s| Some(s.id) == f);
        let mut line = format!("{}{}{text}", "  ".repeat(depth), if foreign { b.path(f) } else { name.to_owned() });
        if let Some(share) = b.share(f).filter(|s| Some(s.folder_id) == f) {
            let (said, share) = shared(&b, share);
            line += &format!(" · {said}");
            v["owner"] = share["owner"].clone();
            v["members"] = share["members"].clone();
        }
        lines.push(line);
        data.push(v);
    }
    Ok(Output { text: lines.join("\n"), data: data.into() })
}

pub fn list(c: &Connection, cx: &Cx, a: ListArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let f = find_folder(&b, a.folder.as_deref().unwrap_or("/"))?;
    let limit = limit(a.limit)?;
    let mut lines = vec![b.path(f)];
    let mut data = json!({ "folder": b.path(f) });
    if let Some((said, share)) = b.share(f).map(|s| shared(&b, s)) {
        lines.push(format!("  {said}"));
        data["owner"] = share["owner"].clone();
        data["members"] = share["members"].clone();
    }
    // The AI jobs here: an agent reading the folder sees that more is on its way, or failed.
    let mut jobs = serde_json::Map::new();
    let mut said = Vec::new();
    for (state, word) in
        [(JobState::Waiting, "等待"), (JobState::Running, "整理中"), (JobState::Failed, "失败"), (JobState::Ready, "建议待确认")]
    {
        match snap.ai_jobs.iter().filter(|j| j.folder_id == f && j.state == state).count() {
            0 => {}
            n => {
                jobs.insert(state.as_str().into(), n.into());
                said.push(format!("{word} {n}"));
            }
        }
    }
    if !jobs.is_empty() {
        lines.push(format!("  AI 整理：{}（用 jobs 查看）", said.join("、")));
        data["jobs"] = jobs.into();
    }
    let mut folders = Vec::new();
    for sub in b.subfolders(f) {
        let (text, pending, ready) = counts(&b, Some(sub.id));
        lines.push(format!("  {}/{text}", sub.name.as_str()));
        folders.push(json!({ "path": b.path(Some(sub.id)), "name": sub.name, "pending": pending, "ready": ready }));
    }
    let mut tasks: Vec<&Task> = b.tasks_in(f).into_iter().filter(|t| a.all || t.done_at.is_none()).collect();
    let total = tasks.len();
    tasks.truncate(limit);
    lines.extend(tasks.iter().map(|t| format!("  {}", task_line(&b, t, false, true, None))));
    if lines.len() == 1 {
        lines.push("  （空）".into());
    }
    data["folders"] = folders.into();
    data["tasks"] = tasks.iter().map(|t| task_view(&b, t)).collect();
    data["total"] = total.into();
    Ok(Output { text: listed(lines, tasks.len(), total).join("\n"), data })
}

pub fn next(c: &Connection, cx: &Cx, a: NextArgs) -> Result<Output, ApiError> {
    let limit = limit(a.limit)?;
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let steps = b.steps();
    let by = b.due_by();
    let mut tasks = b.next();
    if let Some(picked) = picked(&b, a.folders.as_deref())? {
        tasks.retain(|t| picked.contains(&t.id));
    }
    let total = tasks.len();
    tasks.truncate(limit);
    let text = if tasks.is_empty() {
        if a.folders.is_some() { "这些文件夹里暂无可做的事项。" } else { "暂无可做的事项。" }.to_owned()
    } else {
        let lines = tasks
            .iter()
            .map(|t| {
                let line = task_line(&b, t, t.folder_id.is_some(), false, steps.get(&t.id).copied());
                match by.get(&t.id) {
                    Some(&s) => line + &text::due_by(&b, t, s),
                    None => line,
                }
            })
            .collect();
        listed(lines, tasks.len(), total).join("\n")
    };
    let tasks: Vec<Value> = tasks
        .iter()
        .map(|t| {
            let mut v = task_view(&b, t);
            if let Some(step) = steps.get(&t.id) {
                v["step"] = json!(step);
            }
            if let Some(&s) = by.get(&t.id) {
                v["due_by"] = text::due_by_view(s);
            }
            v
        })
        .collect();
    let mut data = json!({ "tasks": tasks, "total": total });
    if let Some((date, time)) = b.next_start() {
        data["next_start"] = text::moment(date, time).into();
    }
    Ok(Output { text, data })
}

/// The tasks in `folders` and the folders inside them; none, which is all of them, when the folders are
/// not given or the root is one, which has those others share too.
fn picked(b: &Board, folders: Option<&[String]>) -> Result<Option<HashSet<TaskId>>, ApiError> {
    let Some(paths) = folders.filter(|paths| !paths.iter().any(|p| b.find_folder(p) == Some(None))) else { return Ok(None) };
    let folders = paths.iter().map(|p| find_folder(b, p)).collect::<Result<Vec<_>, _>>()?;
    Ok(Some(folders.into_iter().flat_map(|f| b.within(f)).map(|t| t.id).collect()))
}

/// How many days `agenda` lists, unless told otherwise.
const AGENDA_DAYS: u32 = 7;

pub fn agenda(c: &Connection, cx: &Cx, a: AgendaArgs) -> Result<Output, ApiError> {
    let days = match a.days.unwrap_or(AGENDA_DAYS) {
        n @ 1..=31 => n,
        _ => return Err(ApiError::invalid("days 要在 1 到 31 之间", Some("days".into()))),
    };
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let picked = picked(&b, a.folders.as_deref())?;
    let listed = |t: &Task| picked.as_ref().is_none_or(|p| p.contains(&t.id));
    let mut agenda = calendar::agenda(&b, days);
    agenda.overdue.retain(|t| listed(t));
    for list in agenda.days.values_mut() {
        list.retain(|d| listed(d.task));
    }
    agenda.days.retain(|_, list| !list.is_empty());
    // A later time is not one to do yet: what holds up the time now says nothing of it.
    let line = |t: &Task, later: bool| {
        let line = task_line(&b, t, t.folder_id.is_some(), !later, None);
        if later { format!("  {line} · 以后的一次") } else { format!("  {line}") }
    };
    let mut lines = Vec::new();
    if !agenda.overdue.is_empty() {
        lines.push("已逾期：".to_owned());
        lines.extend(agenda.overdue.iter().map(|t| line(t, false)));
    }
    for (date, list) in &agenda.days {
        let when = match date.days_after(&b.now().today) {
            0 => "（今天）",
            1 => "（明天）",
            _ => "",
        };
        lines.push(format!("{}{when}：", date.as_str()));
        lines.extend(list.iter().map(|d| line(d.task, d.later)));
    }
    let text = if lines.is_empty() {
        format!("{}今天起 {days} 天没有截止的事项，也没有逾期的。", if a.folders.is_some() { "这些文件夹里" } else { "" })
    } else {
        lines.join("\n")
    };
    let view = |t: &Task, later: bool| {
        let mut v = task_view(&b, t);
        if later {
            v["later"] = true.into();
        }
        v
    };
    let overdue: Vec<Value> = agenda.overdue.iter().map(|t| view(t, false)).collect();
    let days: Vec<Value> = agenda
        .days
        .iter()
        .map(|(date, list)| json!({ "date": date, "tasks": list.iter().map(|d| view(d.task, d.later)).collect::<Vec<_>>() }))
        .collect();
    Ok(Output { text, data: json!({ "overdue": overdue, "days": days }) })
}

pub fn show(c: &Connection, cx: &Cx, a: ShowArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let t = find_task(&b, &a.task)?;
    let mut lines = vec![format!("{} {} {}", b.label(t), status_tag(&b, t), t.title.as_str()), format!("文件夹：{}", b.path(t.folder_id))];
    if !t.assignees.is_empty() {
        lines.push(format!("负责：{}", usernames(&b, &t.assignees).join("、")));
    }
    if let Some(s) = text::start(t) {
        lines.push(format!("开始：{s}"));
    }
    if let Some(deadline) = text::deadline(t) {
        let reminders: Vec<String> = t.reminders().iter().map(text::remind).collect();
        let remind = if reminders.is_empty() { String::new() } else { format!("，提醒：{}", reminders.join("、")) };
        lines.push(format!("截止：{deadline}{remind}"));
    }
    if let Some(done_at) = t.done_at {
        lines.push(format!("{}：{}", if t.dropped { "放弃" } else { "完成" }, ago(cx.now, done_at)));
    }
    if let Some(c) = &t.completions {
        lines.push(format!("完成次数：{}，最近 {}", c.count, c.last.as_str()));
    }
    let mut records: Vec<&Completion> = snap.completions.iter().filter(|r| r.task_id == t.id).collect();
    records.sort_by(|a, b| (&b.done, &b.due).cmp(&(&a.done, &a.due)));
    records.truncate(SHOWN_RECORDS);
    if !records.is_empty() {
        lines.push("最近的完成记录：".into());
        lines.extend(records.iter().map(|r| format!("  {} 完成，截止 {}，{}", r.done.as_str(), r.due.as_str(), lateness(r))));
    }
    let (before, after) = (b.before(t), b.after(t));
    for (label, list) in [("前置", &before), ("后续", &after)] {
        if !list.is_empty() {
            lines.push(format!("{label}："));
            lines.extend(list.iter().map(|l| format!("  {}", mention(&b, l, l.folder_id != t.folder_id))));
        }
    }
    let attachments = b.attachments(t);
    if !attachments.is_empty() {
        lines.push("附件：".into());
        lines.extend(attachments.iter().map(|a| format!("  {} {}", a.id, attachment_line(a))));
    }
    let source = |by: &str, via: &Option<String>| who(&b, by, via.as_deref());
    lines.push(format!("创建：{}，{}", ago(cx.now, t.created_at), source(&t.created_by, &t.created_via)));
    if t.updated_at != t.created_at {
        lines.push(format!("修改：{}，{}", ago(cx.now, t.updated_at), source(&t.updated_by, &t.updated_via)));
    }
    if !t.description.as_str().is_empty() {
        lines.push(String::new());
        lines.push(t.description.as_str().to_owned());
    }
    let mut data = task_view(&b, t);
    data["created_at"] = t.created_at.into();
    data["updated_at"] = t.updated_at.into();
    if !t.description.as_str().is_empty() {
        data["description"] = json!(t.description);
    }
    for (key, via) in [("created_via", &t.created_via), ("updated_via", &t.updated_via)] {
        if let Some(via) = via {
            data[key] = via.as_str().into();
        }
    }
    for (key, by) in [("created_by", &t.created_by), ("updated_by", &t.updated_by)] {
        if by != cx.user {
            data[key] = usernames(&b, std::slice::from_ref(by))[0].into();
        }
    }
    if !attachments.is_empty() {
        data["attachments"] = attachments.iter().map(|a| attachment_view(a)).collect();
    }
    if !records.is_empty() {
        data["completion_records"] = records.iter().map(|r| json!({ "due": r.due, "done": r.done })).collect();
    }
    for (key, list) in [("before", &before), ("after", &after)] {
        if !list.is_empty() {
            data[key] = list.iter().map(|t| task_view(&b, t)).collect();
        }
    }
    Ok(Output { text: lines.join("\n"), data })
}

/// How many completion records `show` lists.
const SHOWN_RECORDS: usize = 5;

/// `按时`, `晚 3 天`, `提前 2 天`: when a repeating task was done, against when it was due.
fn lateness(r: &Completion) -> String {
    match r.done.days_after(&r.due) {
        0 => "按时".into(),
        late if late > 0 => format!("晚 {late} 天"),
        early => format!("提前 {} 天", -early),
    }
}

/// `图片 1.png（image/png，182 KB）`
fn attachment_line(a: &Attachment) -> String {
    format!("{}（{}，{}）", a.name.as_str(), a.media_type.as_str(), text::size(a.size.get()))
}

fn attachment_view(a: &Attachment) -> Value {
    json!({ "id": a.id, "name": a.name, "type": a.media_type, "size": a.size })
}

/// Text an agent can read as it is.
fn is_text(a: &Attachment) -> bool {
    a.media_type.as_str().starts_with("text/") || a.media_type.as_str() == "application/json"
}

pub fn attachment(c: &Connection, cx: &Cx, a: AttachmentArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let at = snap.attachments.iter().find(|x| x.id == a.id).ok_or_else(|| ApiError::not_found("附件不存在"))?;
    let of = match at.holder {
        Holder::Task(t) => text::item(&b, b.task(t).expect("the snapshot has the tasks of its attachments")),
        Holder::Job(_) => "AI 整理请求的附件".into(),
    };
    let mut lines = vec![format!("{}，{of}", attachment_line(at))];
    let mut data = attachment_view(at);
    if showable_image(at) {
        data["image"] = BASE64.encode(cx.files.read(at.id, at.size)?).into();
        lines.push("图片随结果返回".into());
    } else if is_text(at)
        && at.size.get() <= MAX_TEXT
        && let Ok(content) = String::from_utf8(cx.files.read(at.id, at.size)?)
    {
        lines.push(String::new());
        lines.push(content.clone());
        data["text"] = content.into();
    } else {
        lines.push("这个文件不能在这里查看，命令行可以用 download 下载".into());
    }
    Ok(Output { text: lines.join("\n"), data })
}

pub fn search(c: &Connection, cx: &Cx, a: SearchArgs) -> Result<Output, ApiError> {
    let limit = limit(a.limit)?;
    let query = a.query.trim().to_lowercase();
    if query.is_empty() {
        return Err(ApiError::invalid("关键词不能为空", Some("query".into())));
    }
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let mut found: Vec<&Task> = snap
        .tasks
        .iter()
        .filter(|t| t.title.as_str().to_lowercase().contains(&query) || t.description.as_str().to_lowercase().contains(&query))
        .collect();
    b.sort(&mut found);
    found.sort_by_key(|t| t.done_at.is_some());
    let total = found.len();
    found.truncate(limit);
    let text = match total {
        0 => format!("未找到包含“{}”的事项。", a.query.trim()),
        _ => {
            let lines = std::iter::once(format!("找到 {total} 项："))
                .chain(found.iter().map(|t| task_line(&b, t, t.folder_id.is_some(), true, None)));
            listed(lines.collect(), found.len(), total).join("\n")
        }
    };
    let tasks: Vec<Value> = found.iter().map(|t| task_view(&b, t)).collect();
    Ok(Output { text, data: json!({ "tasks": tasks, "total": total }) })
}

pub fn graph(c: &Connection, cx: &Cx, a: GraphArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let scope = find_folder(&b, a.folder.as_deref().unwrap_or("/"))?;
    let shown = |t: &Task| a.all || t.done_at.is_none();
    let mut nodes: Vec<&Task> = snap.tasks.iter().filter(|t| shown(t) && b.in_subtree(t.folder_id, scope)).collect();
    let mut seen: HashSet<_> = nodes.iter().map(|t| t.id).collect();
    for i in 0..nodes.len() {
        let t = nodes[i];
        for n in b.before(t).into_iter().chain(b.after(t)) {
            if shown(n) && seen.insert(n.id) {
                nodes.push(n);
            }
        }
    }
    let nodes = b.order(nodes, shown);
    let edges: Vec<(&Task, &Task)> =
        nodes.iter().flat_map(|&t| b.before(t).into_iter().filter(|p| seen.contains(&p.id)).map(move |p| (p, t))).collect();
    let text = if nodes.is_empty() {
        "暂无待办事项。".to_owned()
    } else if a.mermaid {
        mermaid(&b, &nodes, &edges, scope)
    } else {
        nodes
            .iter()
            .map(|&t| {
                let line = task_line(&b, t, t.folder_id != scope, true, None);
                let before: Vec<String> = edges.iter().filter(|(_, a)| a.id == t.id).map(|(p, _)| b.label(p)).collect();
                if before.is_empty() { line } else { format!("{line} ← {}", before.join(", ")) }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tasks: Vec<Value> = nodes.iter().map(|t| task_view(&b, t)).collect();
    // As tasks are given: by number, and those in folders others share as `@alice#42`.
    let refer = |t: &Task| if b.owner(t) == cx.user { json!(t.number) } else { json!(b.label(t)) };
    let edges: Vec<Value> = edges.iter().map(|(p, t)| json!({ "before": refer(p), "after": refer(t) })).collect();
    Ok(Output { text, data: json!({ "tasks": tasks, "edges": edges }) })
}

fn mermaid(b: &Board, nodes: &[&Task], edges: &[(&Task, &Task)], scope: Option<FolderId>) -> String {
    // Mermaid's entities: `#` first, so that a title cannot spell one out itself.
    let escape = |s: &str| s.replace('#', "#35;").replace('"', "#quot;").replace('<', "#lt;").replace('>', "#gt;").replace('`', "#96;");
    let mut lines = vec!["flowchart LR".to_owned()];
    // Numbers are each owner's: the nodes go by their place in the list.
    let node: HashMap<TaskId, usize> = nodes.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    for (i, t) in nodes.iter().enumerate() {
        let class = match b.status(t) {
            Status::Ready => "ready",
            Status::Waiting { .. } => "waiting",
            Status::Done | Status::Dropped => "done",
        };
        let mut label = format!("{} {}", b.label(t), escape(t.title.as_str()));
        if t.folder_id != scope {
            label += &format!("<br/>{}", escape(&b.path(t.folder_id)));
        }
        lines.push(format!("  t{i}[\"{label}\"]:::{class}"));
    }
    lines.extend(edges.iter().map(|(p, t)| format!("  t{} --> t{}", node[&p.id], node[&t.id])));
    lines.push("  classDef ready fill:#A6F2D1,stroke:#1B6B51,color:#002116".into());
    lines.push("  classDef waiting fill:#FFFFFF,stroke:#707973,stroke-dasharray:4 3,color:#404944".into());
    lines.push("  classDef done fill:#DEE4DF,stroke:#BFC9C2,color:#707973".into());
    lines.join("\n")
}

/// The AI jobs not done yet, oldest first: done ones are tasks by now, but for tidyings with
/// suggestions to take.
pub fn jobs(c: &Connection, cx: &Cx, _: NoArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.user)?;
    if snap.ai_jobs.is_empty() {
        return Ok(Output { text: "没有 AI 整理请求。".into(), data: json!([]) });
    }
    let b = Board::new(&snap, cx.local.clone(), cx.user);
    let (mut lines, mut data) = (Vec::new(), Vec::new());
    for j in &snap.ai_jobs {
        let state = match j.state {
            JobState::Waiting => "等待整理".to_owned(),
            JobState::Running => "正在整理".to_owned(),
            JobState::Failed => format!("整理失败：{}", j.error.as_deref().unwrap_or_default()),
            JobState::Ready => format!("{} 条建议待用户确认", j.suggestions.as_ref().map_or(0, Vec::len)),
        };
        let folder = b.path(j.folder_id);
        lines.push(format!("{} · {folder} · {state} · {}", job_item(j), ago(cx.now, j.updated_at)));
        let mut job =
            json!({ "id": j.id, "kind": j.kind, "start": job_item(j), "folder": folder, "state": j.state, "updated_at": j.updated_at });
        if let Some(error) = &j.error {
            job["error"] = error.as_str().into();
        }
        data.push(job);
    }
    Ok(Output { text: lines.join("\n"), data: data.into() })
}

pub fn log(c: &Connection, cx: &Cx, a: LogArgs) -> Result<Output, ApiError> {
    // One batch in full, or a page of them, each kind of change listing a few items.
    let (limit, before, deleted, items) = match a.batch {
        Some(_) => (1, i64::MAX, false, usize::MAX),
        None => (a.limit.unwrap_or(10), a.before.unwrap_or(i64::MAX), a.deleted, 3),
    };
    if !(1..=100).contains(&limit) {
        return Err(ApiError::invalid("条数要在 1 到 100 之间", Some("limit".into())));
    }
    let mut stmt = c.prepare(
        "SELECT m.seq, m.owner_id, m.actor, t.kind, t.name, m.ops, coalesce(d.effects, m.effects), m.created_at, m.id
         FROM deliveries d JOIN mutations m ON m.seq = d.seq LEFT JOIN tokens t ON t.id = m.token_id
         WHERE d.user_id = ?1 AND m.effects IS NOT NULL AND d.seq < ?3 AND (?5 IS NULL OR d.seq = ?5)
           AND (NOT ?4 OR EXISTS (SELECT 1 FROM json_each(coalesce(d.effects, m.effects)) e
             WHERE json_extract(e.value, '$.kind') IN ('task', 'folder', 'attachment')
               AND json_type(e.value, '$.before') = 'object' AND json_type(e.value, '$.after') = 'null'))
         ORDER BY d.seq DESC LIMIT ?2",
    )?;
    struct Row {
        id: String,
        seq: i64,
        user: String,
        actor: Option<String>,
        device: Option<String>,
        ops: Vec<Op>,
        effects: Vec<Effect>,
        created_at: i64,
    }
    let mut rows = Vec::new();
    for r in stmt.query_map(params![cx.user, limit, before, deleted, a.batch], |r| {
        let kind: Option<String> = r.get(3)?;
        let name: Option<String> = r.get(4)?;
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            name.filter(|_| kind.as_deref() == Some("session")),
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get(7)?,
            r.get(8)?,
        ))
    })? {
        let (seq, user, actor, device, ops, effects, created_at, id) = r?;
        rows.push(Row {
            id,
            seq,
            user,
            actor,
            device,
            ops: serde_json::from_str(&ops)?,
            effects: serde_json::from_str(&effects)?,
            created_at,
        });
    }
    if rows.is_empty() {
        return Ok(Output { text: "暂无改动记录。".into(), data: json!([]) });
    }
    let is_undone = undone(c, cx.user, rows[rows.len() - 1].seq)?;
    let snap = store::snapshot(c, cx.user)?;
    let mut others = text::strangers(&snap, rows.iter().flat_map(|r| &r.effects));
    for r in &rows {
        if !snap.people.iter().any(|p| p.id == r.user) && !others.contains(&r.user) {
            others.push(r.user.clone());
        }
    }
    let mut names = Names::new(&snap, cx.user, store::people(c, &others)?);
    // The folders as they were at each batch: the folder changes of those not read, after it, are
    // stepped back over too, which a later page, the deleted ones and one batch leave out.
    let read: HashSet<i64> = rows.iter().map(|r| r.seq).collect();
    let mut since = folder_changes(c, cx.user, rows[rows.len() - 1].seq)?.into_iter().peekable();
    let (mut lines, mut data) = (Vec::new(), Vec::new());
    for r in &rows {
        while let Some((seq, e)) = since.next_if(|(seq, _)| *seq > r.seq) {
            if !read.contains(&seq) {
                names.undo(&e);
            }
        }
        // In full only for the one batch asked for; the names are stepped back over it once, below.
        let detail = a.batch.is_some().then(|| text::items(&r.effects, &mut names.clone()));
        let changes = text::changes(&r.effects, &mut names, items).0;
        let mut summary = if changes.is_empty() { "没有变化".into() } else { changes.join("；") };
        // As the apps undo: one batch, alone. Several, or along with other changes, read as changes.
        let undoes = match r.ops.as_slice() {
            [Op::Undo { mutation }] => {
                let undoes: Option<i64> = c.query_row("SELECT seq FROM mutations WHERE id = ?1", [mutation], |r| r.get(0)).optional()?;
                summary = format!("撤销{}：{summary}", undoes.map_or(String::new(), |s| format!("批次 {s}")));
                undoes
            }
            _ => None,
        };
        let source = match (&r.device, r.user == cx.user) {
            (Some(device), true) if r.actor.is_none() => format!("本人（{device}）"),
            (_, true) => r.actor.clone().unwrap_or_else(|| "本人".into()),
            (_, false) => source(&names.person(&r.user), r.actor.as_deref()),
        };
        let undone = is_undone(&r.id);
        lines.push(format!("批次 {} · {} · {source}{}", r.seq, ago(cx.now, r.created_at), if undone { " · 已撤销" } else { "" }));
        match &detail {
            Some(detail) => {
                lines.extend(undoes.map(|s| format!("  撤销批次 {s}")));
                if detail.is_empty() {
                    lines.push("  没有变化".into());
                }
                lines.extend(detail.iter().flat_map(|item| item.lines()).map(|l| format!("  {l}")));
            }
            None => lines.push(format!("  {summary}")),
        }
        data.push(LogBatch {
            batch: r.seq,
            user: (r.user != cx.user).then(|| names.username(&r.user)),
            actor: r.actor.clone(),
            source,
            created_at: r.created_at,
            summary,
            changes,
            undoes,
            undone,
            items: detail,
        });
    }
    Ok(Output { text: lines.join("\n"), data: serde_json::to_value(data)? })
}

/// A batch as `log` gives it (docs/agent.md#输出).
#[derive(Serialize, TS)]
#[ts(export)]
pub struct LogBatch {
    batch: i64,
    user: Option<String>,
    actor: Option<String>,
    source: String,
    created_at: i64,
    summary: String,
    changes: Vec<String>,
    undoes: Option<i64>,
    undone: bool,
    /// Each thing it changed, in full, of the one batch asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    items: Option<Vec<LogItem>>,
}

/// Whether a change the user sees is undone: what it did is not there now, as far as undoing goes.
/// The changes to folders user `user` was told of after batch `seq`, by batch, to be stepped back over:
/// the latest first.
fn folder_changes(c: &Connection, user: &str, seq: i64) -> Result<Vec<(i64, Effect)>, ApiError> {
    let mut stmt = c.prepare(
        "SELECT d.seq, e.value FROM deliveries d JOIN mutations m ON m.seq = d.seq, json_each(coalesce(d.effects, m.effects)) e
         WHERE d.user_id = ?1 AND d.seq > ?2 AND json_extract(e.value, '$.kind') = 'folder'
         ORDER BY d.seq DESC, e.key DESC",
    )?;
    let mut found = Vec::new();
    for r in stmt.query_map(params![user, seq], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))? {
        let (seq, e) = r?;
        found.push((seq, serde_json::from_str(&e)?));
    }
    Ok(found)
}

/// A change and the undos of it, each a batch undoing that one change alone, make a line, each
/// turning it over: a change is undone when it goes the other way from the latest in its line, so
/// undoing a change again after its undo was undone turns that undo of the undo back too. A batch
/// undoing several changes, or doing more besides, which only the API makes, undoes them while it is
/// not undone itself: which of them it changed is not known. Only the undos from batch `from` on can
/// be the latest of a line with a change from there on in it: what undoes a change comes after it.
fn undone(c: &Connection, user: &str, from: i64) -> Result<impl Fn(&str) -> bool, ApiError> {
    // An undo that changed nothing, of a change undone already, undoes nothing.
    const UNDOING: &str = "SELECT m.id, json_extract(o.value, '$.mutation'), json_array_length(m.ops)
         FROM deliveries d JOIN mutations m ON m.seq = d.seq, json_each(m.ops) o
         WHERE d.user_id = ?1 AND json_array_length(m.effects) > 0 AND json_extract(o.value, '$.op') = 'undo'";
    // What each undo of one change alone undoes, and those from `from` on in order; the batches
    // undoing several, by what they undo.
    let mut parent: HashMap<String, Option<String>> = HashMap::new();
    let mut order = Vec::new();
    let mut by: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt = c.prepare(&format!("{UNDOING} AND d.seq >= ?2 ORDER BY d.seq"))?;
    for r in stmt.query_map(params![user, from], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
        let (undoing, undone, ops) = r?;
        if ops == 1 {
            order.push(undoing.clone());
            parent.insert(undoing, Some(undone));
        } else {
            by.entry(undone).or_default().push(undoing);
        }
    }
    // Where each line of those starts, and how far along it each is: lines run through undos from
    // before `from` too.
    let mut older = c.prepare(&format!("{UNDOING} AND m.id = ?2 AND json_array_length(m.ops) = 1"))?;
    let mut place: HashMap<String, (String, usize)> = HashMap::new();
    for undo in &order {
        // Up to where it starts, or to one placed already, placing those on the way after.
        let (mut way, mut at) = (Vec::new(), undo.clone());
        let (start, mut far) = loop {
            if let Some((start, far)) = place.get(&at) {
                break (start.clone(), *far);
            }
            let up = match parent.get(&at) {
                Some(up) => up.clone(),
                None => {
                    let up = older.query_row(params![user, at], |r| r.get::<_, String>(1)).optional()?;
                    parent.insert(at.clone(), up.clone());
                    up
                }
            };
            let Some(up) = up else { break (at, 0) };
            way.push(std::mem::replace(&mut at, up));
        };
        for id in way.into_iter().rev() {
            far += 1;
            place.insert(id, (start.clone(), far));
        }
    }
    // Which way each line goes now: as the latest in it.
    let now: HashMap<String, usize> = order.iter().map(|u| (place[u].0.clone(), place[u].1 % 2)).collect();
    fn undone(id: &str, place: &HashMap<String, (String, usize)>, now: &HashMap<String, usize>, by: &HashMap<String, Vec<String>>) -> bool {
        let (start, far) = place.get(id).map_or((id, 0), |(start, far)| (start.as_str(), *far));
        now.get(start).is_some_and(|way| *way != far % 2) || by.get(id).is_some_and(|all| all.iter().any(|b| !undone(b, place, now, by)))
    }
    Ok(move |id: &str| undone(id, &place, &now, &by))
}

/// Who made a change: `本人`, the token's name, or someone else by name, with their token's:
/// `Bob 的 Claude Code`.
fn who(b: &Board, user: &str, via: Option<&str>) -> String {
    if user == b.me() {
        return via.unwrap_or("本人").to_owned();
    }
    source(b.person(user).map_or(user, |p| p.display_name.as_str()), via)
}

/// Someone else's change, by name: `Bob`, or by their token: `Bob 的 Claude Code`.
fn source(name: &str, via: Option<&str>) -> String {
    match via {
        Some(token) => format!("{name} 的 {token}"),
        None => name.to_owned(),
    }
}
