//! Tools that only read.

use std::collections::HashSet;

use nexttodo_core::board::{Board, Status};
use nexttodo_core::model::{Effect, FolderId, Task};
use nexttodo_core::ops::Op;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::args::{GraphArgs, ListArgs, LogArgs, NoArgs, SearchArgs, ShowArgs};
use super::text::{self, Names, ago, status_tag, task_line, task_view};
use super::{Cx, Output, find_folder, find_task};
use crate::error::ApiError;
use crate::store;

const SEARCH_LIMIT: usize = 50;

/// `（待办 3，可做 1）` for the tasks directly in a folder, and the same as data.
fn counts(b: &Board, f: Option<FolderId>) -> (String, usize, usize) {
    let tasks = b.contents(f);
    let pending = tasks.iter().filter(|t| t.done_at.is_none()).count();
    let ready = tasks.iter().filter(|t| b.status(t) == Status::Ready).count();
    let text = if pending == 0 { "（无待办）".to_owned() } else { format!("（待办 {pending}，可做 {ready}）") };
    (text, pending, ready)
}

pub fn tree(c: &Connection, cx: &Cx, _: NoArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.owner)?;
    let b = Board::new(&snap);
    let (mut lines, mut data) = (Vec::new(), Vec::new());
    for f in b.folder_tree() {
        let name = f.and_then(|id| b.folder(id)).map_or("/", |f| f.name.as_str());
        let depth = std::iter::successors(f, |id| b.folder(*id).and_then(|f| f.parent_id)).count();
        let (text, pending, ready) = counts(&b, f);
        lines.push(format!("{}{name}{text}", "  ".repeat(depth)));
        data.push(json!({ "path": b.path(f), "name": name, "depth": depth, "pending": pending, "ready": ready }));
    }
    Ok(Output { text: lines.join("\n"), data: data.into() })
}

pub fn list(c: &Connection, cx: &Cx, a: ListArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.owner)?;
    let b = Board::new(&snap);
    let f = find_folder(&b, a.folder.as_deref().unwrap_or("/"))?;
    let mut lines = vec![b.path(f)];
    let mut folders = Vec::new();
    for sub in b.subfolders(f) {
        let (text, pending, ready) = counts(&b, Some(sub.id));
        lines.push(format!("  {}/{text}", sub.name.as_str()));
        folders.push(json!({ "path": b.path(Some(sub.id)), "name": sub.name, "pending": pending, "ready": ready }));
    }
    let tasks: Vec<&Task> = b.tasks_in(f).into_iter().filter(|t| a.all || t.done_at.is_none()).collect();
    lines.extend(tasks.iter().map(|t| format!("  {}", task_line(&b, t, false, true))));
    if lines.len() == 1 {
        lines.push("  （空）".into());
    }
    let tasks: Vec<Value> = tasks.iter().map(|t| task_view(&b, t)).collect();
    Ok(Output { text: lines.join("\n"), data: json!({ "folder": b.path(f), "folders": folders, "tasks": tasks }) })
}

pub fn next(c: &Connection, cx: &Cx, _: NoArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.owner)?;
    let b = Board::new(&snap);
    let tasks = b.next();
    let text = if tasks.is_empty() {
        "暂无可做的事项。".to_owned()
    } else {
        tasks.iter().map(|t| task_line(&b, t, t.folder_id.is_some(), false)).collect::<Vec<_>>().join("\n")
    };
    Ok(Output { text, data: tasks.iter().map(|t| task_view(&b, t)).collect() })
}

pub fn show(c: &Connection, cx: &Cx, a: ShowArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.owner)?;
    let b = Board::new(&snap);
    let t = find_task(&b, a.task)?;
    let mut lines =
        vec![format!("#{} {} {}", t.number, status_tag(b.status(t)), t.title.as_str()), format!("文件夹：{}", b.path(t.folder_id))];
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
    let (before, after) = (b.before(t), b.after(t));
    for (label, list) in [("前置", &before), ("后续", &after)] {
        if !list.is_empty() {
            lines.push(format!("{label}："));
            lines.extend(list.iter().map(|l| format!("  {}", task_line(&b, l, l.folder_id != t.folder_id, true))));
        }
    }
    let source = |via: &Option<String>| via.as_deref().unwrap_or("本人").to_owned();
    lines.push(format!("创建：{}，{}", ago(cx.now, t.created_at), source(&t.created_via)));
    if t.updated_at != t.created_at {
        lines.push(format!("修改：{}，{}", ago(cx.now, t.updated_at), source(&t.updated_via)));
    }
    if !t.description.as_str().is_empty() {
        lines.push(String::new());
        lines.push(t.description.as_str().to_owned());
    }
    let mut data = task_view(&b, t);
    data["description"] = json!(t.description);
    data["created_at"] = json!(t.created_at);
    data["updated_at"] = json!(t.updated_at);
    data["created_via"] = json!(t.created_via);
    data["updated_via"] = json!(t.updated_via);
    data["before"] = before.iter().map(|t| task_view(&b, t)).collect();
    data["after"] = after.iter().map(|t| task_view(&b, t)).collect();
    Ok(Output { text: lines.join("\n"), data })
}

pub fn search(c: &Connection, cx: &Cx, a: SearchArgs) -> Result<Output, ApiError> {
    let query = a.query.trim().to_lowercase();
    if query.is_empty() {
        return Err(ApiError::invalid("关键词不能为空", Some("query".into())));
    }
    let snap = store::snapshot(c, cx.owner)?;
    let b = Board::new(&snap);
    let mut found: Vec<&Task> = snap
        .tasks
        .iter()
        .filter(|t| t.title.as_str().to_lowercase().contains(&query) || t.description.as_str().to_lowercase().contains(&query))
        .collect();
    b.sort(&mut found);
    found.sort_by_key(|t| t.done_at.is_some());
    let total = found.len();
    found.truncate(SEARCH_LIMIT);
    let text = match total {
        0 => format!("未找到包含“{}”的事项。", a.query.trim()),
        _ => {
            let head = if total > SEARCH_LIMIT {
                format!("找到 {total} 项，只列出前 {SEARCH_LIMIT} 项：")
            } else {
                format!("找到 {total} 项：")
            };
            std::iter::once(head).chain(found.iter().map(|t| task_line(&b, t, t.folder_id.is_some(), true))).collect::<Vec<_>>().join("\n")
        }
    };
    let tasks: Vec<Value> = found.iter().map(|t| task_view(&b, t)).collect();
    Ok(Output { text, data: json!({ "total": total, "tasks": tasks }) })
}

pub fn graph(c: &Connection, cx: &Cx, a: GraphArgs) -> Result<Output, ApiError> {
    let snap = store::snapshot(c, cx.owner)?;
    let b = Board::new(&snap);
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
                let line = task_line(&b, t, t.folder_id != scope, true);
                let before: Vec<String> = edges.iter().filter(|(_, a)| a.id == t.id).map(|(p, _)| format!("#{}", p.number)).collect();
                if before.is_empty() { line } else { format!("{line} ← {}", before.join(", ")) }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tasks: Vec<Value> = nodes.iter().map(|t| task_view(&b, t)).collect();
    let edges: Vec<Value> = edges.iter().map(|(p, t)| json!({ "before": p.number, "after": t.number })).collect();
    Ok(Output { text, data: json!({ "tasks": tasks, "edges": edges }) })
}

fn mermaid(b: &Board, nodes: &[&Task], edges: &[(&Task, &Task)], scope: Option<FolderId>) -> String {
    // Mermaid's entities: `#` first, so that a title cannot spell one out itself.
    let escape = |s: &str| s.replace('#', "#35;").replace('"', "#quot;").replace('<', "#lt;").replace('>', "#gt;").replace('`', "#96;");
    let mut lines = vec!["flowchart LR".to_owned()];
    for t in nodes {
        let class = match b.status(t) {
            Status::Ready => "ready",
            Status::Waiting(_) => "waiting",
            Status::Done => "done",
        };
        let mut label = format!("#{} {}", t.number, escape(t.title.as_str()));
        if t.folder_id != scope {
            label += &format!("<br/>{}", escape(&b.path(t.folder_id)));
        }
        lines.push(format!("  t{}[\"{label}\"]:::{class}", t.number));
    }
    lines.extend(edges.iter().map(|(p, t)| format!("  t{} --> t{}", p.number, t.number)));
    lines.push("  classDef ready fill:#A6F2D1,stroke:#1B6B51,color:#002116".into());
    lines.push("  classDef waiting fill:#FFFFFF,stroke:#707973,stroke-dasharray:4 3,color:#404944".into());
    lines.push("  classDef done fill:#DEE4DF,stroke:#BFC9C2,color:#707973".into());
    lines.join("\n")
}

pub fn log(c: &Connection, cx: &Cx, a: LogArgs) -> Result<Output, ApiError> {
    let limit = a.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::invalid("条数要在 1 到 100 之间", Some("limit".into())));
    }
    let mut stmt = c.prepare(
        "SELECT m.seq, m.actor, t.kind, t.name, m.ops, m.effects, m.created_at FROM mutations m LEFT JOIN tokens t ON t.id = m.token_id
         WHERE m.owner_id = ?1 AND m.effects IS NOT NULL ORDER BY m.seq DESC LIMIT ?2",
    )?;
    struct Row {
        seq: i64,
        actor: Option<String>,
        device: Option<String>,
        ops: Vec<Op>,
        effects: Vec<Effect>,
        created_at: i64,
    }
    let mut rows = Vec::new();
    for r in stmt.query_map(params![cx.owner, limit], |r| {
        let kind: Option<String> = r.get(2)?;
        let name: Option<String> = r.get(3)?;
        Ok((
            r.get(0)?,
            r.get(1)?,
            name.filter(|_| kind.as_deref() == Some("session")),
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get(6)?,
        ))
    })? {
        let (seq, actor, device, ops, effects, created_at) = r?;
        rows.push(Row { seq, actor, device, ops: serde_json::from_str(&ops)?, effects: serde_json::from_str(&effects)?, created_at });
    }
    if rows.is_empty() {
        return Ok(Output { text: "暂无改动记录。".into(), data: json!([]) });
    }
    let snap = store::snapshot(c, cx.owner)?;
    let mut names = Names::new(&snap);
    let (mut lines, mut data) = (Vec::new(), Vec::new());
    for r in &rows {
        let mut summary = text::changes(&r.effects, &mut names, 3).join("；");
        if summary.is_empty() {
            summary = "没有变化".into();
        }
        if let Some(Op::Undo { mutation }) = r.ops.first() {
            let undone: Option<i64> = c
                .query_row("SELECT seq FROM mutations WHERE id = ?1 AND owner_id = ?2", params![mutation, cx.owner], |r| r.get(0))
                .optional()?;
            summary = format!("撤销{}：{summary}", undone.map_or(String::new(), |s| format!("批次 {s}")));
        }
        let who = match (&r.actor, &r.device) {
            (Some(token), _) => token.clone(),
            (None, Some(device)) => format!("本人（{device}）"),
            (None, None) => "本人".into(),
        };
        lines.push(format!("批次 {} · {} · {who}", r.seq, ago(cx.now, r.created_at)));
        lines.push(format!("  {summary}"));
        data.push(json!({ "batch": r.seq, "actor": r.actor, "created_at": r.created_at, "summary": summary }));
    }
    Ok(Output { text: lines.join("\n"), data: data.into() })
}
