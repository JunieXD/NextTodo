//! Tools that only read.

use std::collections::{HashMap, HashSet};

use nexttodo_core::board::{Board, Status};
use nexttodo_core::model::{Effect, FolderId, JobState, Share, Task, TaskId};
use nexttodo_core::ops::Op;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::args::{GraphArgs, ListArgs, LogArgs, NextArgs, NoArgs, SearchArgs, ShowArgs};
use super::text::{self, Names, ago, job_item, mention, status_tag, task_line, task_view, usernames};
use super::{Cx, Output, find_folder, find_task};
use crate::error::ApiError;
use crate::store;

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

/// `（待办 3，可做 1）` for the tasks directly in a folder, and the same as data.
fn counts(b: &Board, f: Option<FolderId>) -> (String, usize, usize) {
    let tasks = b.contents(f);
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
    let b = Board::new(&snap, cx.today.clone(), cx.user);
    let (mut lines, mut data) = (Vec::new(), Vec::new());
    for f in b.folder_tree() {
        let name = f.and_then(|id| b.folder(id)).map_or("/", |f| f.name.as_str());
        let depth = std::iter::successors(f, |id| b.folder(*id).and_then(|f| f.parent_id)).count();
        let (text, pending, ready) = counts(&b, f);
        let mut v = json!({ "path": b.path(f), "name": name, "depth": depth, "pending": pending, "ready": ready });
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
    let b = Board::new(&snap, cx.today.clone(), cx.user);
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
    let b = Board::new(&snap, cx.today.clone(), cx.user);
    let steps = b.steps();
    let mut tasks = b.next();
    let total = tasks.len();
    tasks.truncate(limit);
    let text = if tasks.is_empty() {
        "暂无可做的事项。".to_owned()
    } else {
        let lines = tasks.iter().map(|t| task_line(&b, t, t.folder_id.is_some(), false, steps.get(&t.id).copied())).collect();
        listed(lines, tasks.len(), total).join("\n")
    };
    let tasks: Vec<Value> = tasks
        .iter()
        .map(|t| {
            let mut v = task_view(&b, t);
            if let Some(step) = steps.get(&t.id) {
                v["step"] = json!(step);
            }
            v
        })
        .collect();
    Ok(Output { text, data: json!({ "tasks": tasks, "total": total }) })
}

pub fn show(c: &Connection, cx: &Cx, a: ShowArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.user)?;
    let b = Board::new(&snap, cx.today.clone(), cx.user);
    let t = find_task(&b, &a.task)?;
    let mut lines = vec![format!("{} {} {}", b.label(t), status_tag(&b, t), t.title.as_str()), format!("文件夹：{}", b.path(t.folder_id))];
    if !t.assignees.is_empty() {
        lines.push(format!("负责：{}", usernames(&b, &t.assignees).join("、")));
    }
    if let Some(s) = &t.start {
        lines.push(format!("开始：{}", s.as_str()));
    }
    if let Some(d) = &t.due {
        let remind = match d.at.as_ref().and_then(|at| at.remind_before) {
            None => String::new(),
            Some(0) => "，截止时提醒".into(),
            Some(m) => format!("，提前 {m} 分钟提醒"),
        };
        lines.push(format!("截止：{}{remind}", text::due(d)));
    }
    if let Some(done_at) = t.done_at {
        lines.push(format!("完成：{}", ago(cx.now, done_at)));
    }
    if let Some(c) = &t.completions {
        lines.push(format!("完成次数：{}，最近 {}", c.count, c.last.as_str()));
    }
    let (before, after) = (b.before(t), b.after(t));
    for (label, list) in [("前置", &before), ("后续", &after)] {
        if !list.is_empty() {
            lines.push(format!("{label}："));
            lines.extend(list.iter().map(|l| format!("  {}", mention(&b, l, l.folder_id != t.folder_id))));
        }
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
    for (key, list) in [("before", &before), ("after", &after)] {
        if !list.is_empty() {
            data[key] = list.iter().map(|t| task_view(&b, t)).collect();
        }
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
    let b = Board::new(&snap, cx.today.clone(), cx.user);
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
    let b = Board::new(&snap, cx.today.clone(), cx.user);
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
            Status::Done => "done",
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
    let b = Board::new(&snap, cx.today.clone(), cx.user);
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
    let limit = a.limit.unwrap_or(10);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::invalid("条数要在 1 到 100 之间", Some("limit".into())));
    }
    let mut stmt = c.prepare(
        "SELECT m.seq, m.owner_id, m.actor, t.kind, t.name, m.ops, coalesce(d.effects, m.effects), m.created_at
         FROM deliveries d JOIN mutations m ON m.seq = d.seq LEFT JOIN tokens t ON t.id = m.token_id
         WHERE d.user_id = ?1 AND m.effects IS NOT NULL ORDER BY d.seq DESC LIMIT ?2",
    )?;
    struct Row {
        seq: i64,
        user: String,
        actor: Option<String>,
        device: Option<String>,
        ops: Vec<Op>,
        effects: Vec<Effect>,
        created_at: i64,
    }
    let mut rows = Vec::new();
    for r in stmt.query_map(params![cx.user, limit], |r| {
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
        ))
    })? {
        let (seq, user, actor, device, ops, effects, created_at) = r?;
        rows.push(Row { seq, user, actor, device, ops: serde_json::from_str(&ops)?, effects: serde_json::from_str(&effects)?, created_at });
    }
    if rows.is_empty() {
        return Ok(Output { text: "暂无改动记录。".into(), data: json!([]) });
    }
    let snap = store::snapshot(c, cx.user)?;
    let mut others = text::strangers(&snap, rows.iter().flat_map(|r| &r.effects));
    for r in &rows {
        if !snap.people.iter().any(|p| p.id == r.user) && !others.contains(&r.user) {
            others.push(r.user.clone());
        }
    }
    let mut names = Names::new(&snap, cx.user, store::people(c, &others)?);
    let (mut lines, mut data) = (Vec::new(), Vec::new());
    for r in &rows {
        let mut summary = text::changes(&r.effects, &mut names, 3).0.join("；");
        if summary.is_empty() {
            summary = "没有变化".into();
        }
        if let Some(Op::Undo { mutation }) = r.ops.first() {
            let undone: Option<i64> = c.query_row("SELECT seq FROM mutations WHERE id = ?1", [mutation], |r| r.get(0)).optional()?;
            summary = format!("撤销{}：{summary}", undone.map_or(String::new(), |s| format!("批次 {s}")));
        }
        let source = match (&r.device, r.user == cx.user) {
            (Some(device), true) if r.actor.is_none() => format!("本人（{device}）"),
            (_, true) => r.actor.clone().unwrap_or_else(|| "本人".into()),
            (_, false) => source(&names.person(&r.user), r.actor.as_deref()),
        };
        lines.push(format!("批次 {} · {} · {source}", r.seq, ago(cx.now, r.created_at)));
        lines.push(format!("  {summary}"));
        let user = (r.user != cx.user).then(|| names.username(&r.user));
        data.push(json!({ "batch": r.seq, "user": user, "actor": r.actor, "created_at": r.created_at, "summary": summary }));
    }
    Ok(Output { text: lines.join("\n"), data: data.into() })
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
