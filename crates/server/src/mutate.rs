//! The mutation pipeline: validate and apply operations in one transaction, record the resulting
//! entity changes in the mutation log. Resubmitting a mutation id returns the first result.

use nexttodo_core::dag::find_path;
use nexttodo_core::model::{Change, Dependency, Effect, Folder, FolderId, Link, MutationId, SortKey, Task, TaskId};
use nexttodo_core::ops::{Mutation, Op};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use ts_rs::TS;

use crate::error::ApiError;
use crate::store;

/// Who is making the change.
pub struct Actor<'a> {
    pub owner: &'a str,
    pub token_id: &'a str,
    /// API token name, recorded as the source; `None` for the user's own devices.
    pub via: Option<&'a str>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Applied {
    pub seq: i64,
    pub effects: Vec<Effect>,
}

pub fn apply(conn: &mut Connection, actor: &Actor, m: &Mutation, now: i64) -> Result<Applied, ApiError> {
    let tx = conn.transaction()?;
    if let Some(applied) = earlier(&tx, actor, m.id)? {
        return Ok(applied);
    }
    let effects = run(&tx, actor, &m.ops, now)?;
    let seq = record(&tx, actor, m.id, &m.ops, &effects, now)?;
    tx.commit()?;
    Ok(Applied { seq, effects })
}

/// The result of a mutation that was already applied.
pub fn earlier(c: &Connection, actor: &Actor, id: MutationId) -> Result<Option<Applied>, ApiError> {
    let row: Option<(String, i64, Option<String>)> = c
        .query_row("SELECT owner_id, seq, effects FROM mutations WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((owner, seq, effects)) = row else { return Ok(None) };
    if owner != actor.owner {
        return Err(ApiError::conflict("改动 ID 重复"));
    }
    // past the retention period only the receipt is left
    let effects = effects.map(|e| serde_json::from_str(&e)).transpose()?.unwrap_or_default();
    Ok(Some(Applied { seq, effects }))
}

/// Applies the operations inside the caller's transaction and returns what changed.
pub fn run(c: &Connection, actor: &Actor, ops: &[Op], now: i64) -> Result<Vec<Effect>, ApiError> {
    let mut cx = Cx { c, owner: actor.owner, via: actor.via.map(str::to_owned), now, effects: Vec::new() };
    for op in ops {
        cx.op(op)?;
    }
    Ok(cx.effects)
}

/// Adds the mutation to the log and returns its sequence number.
pub fn record(c: &Connection, actor: &Actor, id: MutationId, ops: &[Op], effects: &[Effect], now: i64) -> Result<i64, ApiError> {
    c.execute(
        "INSERT INTO mutations (id, owner_id, token_id, actor, ops, effects, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, actor.owner, actor.token_id, actor.via, serde_json::to_string(ops)?, serde_json::to_string(effects)?, now],
    )?;
    Ok(c.last_insert_rowid())
}

/// `cur` with the fields that changed from `before` to `after` set back to `before`, so that undoing
/// a change leaves later changes to other fields alone.
macro_rules! restore {
    ($cur:expr, $before:expr, $after:expr, $($field:ident),*) => {{
        let mut restored = $cur.clone();
        $(if $before.$field != $after.$field {
            restored.$field = $before.$field.clone();
        })*
        restored
    }};
}

struct Cx<'a> {
    c: &'a Connection,
    owner: &'a str,
    via: Option<String>,
    now: i64,
    effects: Vec<Effect>,
}

impl Cx<'_> {
    fn op(&mut self, op: &Op) -> Result<(), ApiError> {
        match op {
            Op::FolderCreate { id, name, parent_id, sort_key } => {
                if store::folder_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("文件夹 ID 已存在"));
                }
                if let Some(p) = parent_id {
                    self.folder(*p)?;
                }
                let sort_key = match sort_key {
                    Some(k) => k.clone(),
                    None => SortKey::after(store::last_folder_key(self.c, self.owner, *parent_id)?.as_ref()),
                };
                self.create_folder(Folder {
                    id: *id,
                    parent_id: *parent_id,
                    name: name.clone(),
                    sort_key,
                    created_at: self.now,
                    updated_at: self.now,
                })
            }
            Op::FolderUpdate { id, name, parent_id, sort_key } => {
                let before = self.folder(*id)?;
                let mut f = before.clone();
                if let Some(name) = name {
                    f.name = name.clone();
                }
                if let Some(parent_id) = parent_id {
                    f.parent_id = *parent_id;
                }
                if let Some(k) = sort_key {
                    f.sort_key = k.clone();
                }
                self.update_folder(before, f)
            }
            Op::FolderDelete { id } => {
                self.folder(*id)?;
                self.delete_folder(*id)
            }
            Op::TaskCreate { id, title, folder_id, description, due, sort_key, done } => {
                if store::task_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("事项 ID 已存在"));
                }
                if let Some(f) = folder_id {
                    self.folder(*f)?;
                }
                let sort_key = match sort_key {
                    Some(k) => k.clone(),
                    None => SortKey::after(store::last_task_key(self.c, self.owner, *folder_id)?.as_ref()),
                };
                let task = Task {
                    id: *id,
                    number: store::take_task_number(self.c, self.owner)?,
                    folder_id: *folder_id,
                    title: title.clone(),
                    description: description.clone(),
                    done_at: done.then_some(self.now),
                    due: due.clone(),
                    sort_key,
                    created_at: self.now,
                    updated_at: self.now,
                    created_via: self.via.clone(),
                    updated_via: self.via.clone(),
                };
                self.create_task(task)
            }
            Op::TaskUpdate { id, folder_id, title, description, due, sort_key, done } => {
                let before = self.task(*id)?;
                let mut t = before.clone();
                if let Some(folder_id) = folder_id {
                    t.folder_id = *folder_id;
                }
                if let Some(title) = title {
                    t.title = title.clone();
                }
                if let Some(description) = description {
                    t.description = description.clone();
                }
                if let Some(due) = due {
                    t.due = due.clone();
                }
                if let Some(k) = sort_key {
                    t.sort_key = k.clone();
                }
                if let Some(done) = done
                    && *done != t.done_at.is_some()
                {
                    t.done_at = done.then_some(self.now);
                }
                self.update_task(before, t)
            }
            Op::TaskDelete { id } => {
                self.task(*id)?;
                self.delete_task(*id)
            }
            Op::DepAdd { before, after } => {
                if before == after {
                    return Err(ApiError::invalid("事项不能依赖自己", None));
                }
                self.task(*before)?;
                self.task(*after)?;
                if store::load_dependency(self.c, self.owner, *before, *after)?.is_some() {
                    return Err(ApiError::conflict("该依赖已存在"));
                }
                self.create_dependency(Dependency { before: *before, after: *after, created_at: self.now })
            }
            Op::DepRemove { before, after } => {
                let d = store::load_dependency(self.c, self.owner, *before, *after)?.ok_or_else(|| ApiError::not_found("该依赖不存在"))?;
                self.delete_dependency(d)
            }
            Op::Undo { mutation } => self.undo(*mutation),
        }
    }

    fn folder(&self, id: FolderId) -> Result<Folder, ApiError> {
        store::load_folder(self.c, self.owner, id)?.ok_or_else(|| ApiError::not_found("文件夹不存在"))
    }

    fn task(&self, id: TaskId) -> Result<Task, ApiError> {
        store::load_task(self.c, self.owner, id)?.ok_or_else(|| ApiError::not_found("事项不存在"))
    }

    fn check_name(&self, f: &Folder) -> Result<(), ApiError> {
        if store::folder_name_taken(self.c, self.owner, f)? {
            return Err(ApiError::conflict(format!("同一层级已有名为“{}”的文件夹", f.name.as_str())));
        }
        Ok(())
    }

    fn create_folder(&mut self, f: Folder) -> Result<(), ApiError> {
        self.check_name(&f)?;
        store::insert_folder(self.c, self.owner, &f)?;
        self.effects.push(Effect::Folder(Change::Created(f)));
        Ok(())
    }

    fn update_folder(&mut self, before: Folder, mut after: Folder) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        if after.parent_id != before.parent_id
            && let Some(p) = after.parent_id
        {
            self.folder(p)?;
            if store::in_subtree(self.c, after.id, p)? {
                return Err(ApiError::folder_cycle());
            }
        }
        if after.parent_id != before.parent_id || after.name != before.name {
            self.check_name(&after)?;
        }
        after.updated_at = self.now;
        store::update_folder(self.c, &after)?;
        self.effects.push(Effect::Folder(Change::Updated { before, after }));
        Ok(())
    }

    /// Records the removal of everything in the subtree (dependencies, tasks, folders deepest first)
    /// so that undo can bring it all back in reverse order. Folders are deleted one by one, deepest
    /// first, because a single cascade through a deep tree would hit SQLite's trigger depth limit.
    fn delete_folder(&mut self, id: FolderId) -> Result<(), ApiError> {
        for d in store::subtree_dependencies(self.c, id)? {
            self.effects.push(Effect::Dependency(Link::Removed(d)));
        }
        for t in store::subtree_tasks(self.c, id)? {
            self.effects.push(Effect::Task(Change::Deleted(Box::new(t))));
        }
        for f in store::subtree_folders(self.c, id)? {
            store::delete_folder(self.c, f.id)?;
            self.effects.push(Effect::Folder(Change::Deleted(f)));
        }
        Ok(())
    }

    fn create_task(&mut self, t: Task) -> Result<(), ApiError> {
        store::insert_task(self.c, self.owner, &t)?;
        self.effects.push(Effect::Task(Change::Created(Box::new(t))));
        Ok(())
    }

    fn update_task(&mut self, before: Task, mut after: Task) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        if after.folder_id != before.folder_id
            && let Some(f) = after.folder_id
        {
            self.folder(f)?;
        }
        after.updated_at = self.now;
        after.updated_via = self.via.clone();
        store::update_task(self.c, &after)?;
        self.effects.push(Effect::Task(Change::Updated { before: Box::new(before), after: Box::new(after) }));
        Ok(())
    }

    fn delete_task(&mut self, id: TaskId) -> Result<(), ApiError> {
        for d in store::task_dependencies(self.c, id)? {
            self.effects.push(Effect::Dependency(Link::Removed(d)));
        }
        let t = self.task(id)?;
        store::delete_task(self.c, id)?;
        self.effects.push(Effect::Task(Change::Deleted(Box::new(t))));
        Ok(())
    }

    fn create_dependency(&mut self, d: Dependency) -> Result<(), ApiError> {
        let edges = store::dependency_edges(self.c, self.owner)?;
        if let Some(path) = find_path(&edges, d.after, d.before) {
            let path: Vec<TaskId> = [d.before].into_iter().chain(path).collect();
            let titles = path.iter().map(|&id| Ok(format!("“{}”", self.task(id)?.title.as_str()))).collect::<Result<Vec<_>, ApiError>>()?;
            return Err(ApiError::cycle(path, &titles.join(" → ")));
        }
        store::insert_dependency(self.c, self.owner, &d)?;
        self.effects.push(Effect::Dependency(Link::Added(d)));
        Ok(())
    }

    fn delete_dependency(&mut self, d: Dependency) -> Result<(), ApiError> {
        store::delete_dependency(self.c, &d)?;
        self.effects.push(Effect::Dependency(Link::Removed(d)));
        Ok(())
    }

    /// Restores what a mutation changed, newest change first. Entities that were removed come back
    /// unchanged (same id and number). Changes whose target is already gone or back are skipped;
    /// bringing something back into a folder or onto a task that is gone fails.
    fn undo(&mut self, id: MutationId) -> Result<(), ApiError> {
        let effects: String = self
            .c
            .query_row("SELECT effects FROM mutations WHERE id = ?1 AND owner_id = ?2", params![id, self.owner], |r| r.get(0))
            .optional()?
            .flatten()
            .ok_or_else(|| ApiError::not_found("要撤销的改动不存在或已过期"))?;
        let effects: Vec<Effect> = serde_json::from_str(&effects)?;
        for effect in effects.into_iter().rev() {
            match effect {
                Effect::Folder(Change::Created(f)) => {
                    if store::load_folder(self.c, self.owner, f.id)?.is_some() {
                        self.delete_folder(f.id)?;
                    }
                }
                Effect::Folder(Change::Deleted(f)) => {
                    if store::load_folder(self.c, self.owner, f.id)?.is_none() {
                        if let Some(p) = f.parent_id
                            && store::load_folder(self.c, self.owner, p)?.is_none()
                        {
                            return Err(ApiError::not_found(format!("文件夹“{}”原所在的文件夹已删除", f.name.as_str())));
                        }
                        self.create_folder(Folder { updated_at: self.now, ..f })?;
                    }
                }
                Effect::Folder(Change::Updated { before: b, after: a }) => {
                    if let Some(cur) = store::load_folder(self.c, self.owner, b.id)? {
                        let restored = restore!(cur, b, a, parent_id, name, sort_key);
                        self.update_folder(cur, restored)?;
                    }
                }
                Effect::Task(Change::Created(t)) => {
                    if store::load_task(self.c, self.owner, t.id)?.is_some() {
                        self.delete_task(t.id)?;
                    }
                }
                Effect::Task(Change::Deleted(t)) => {
                    if store::load_task(self.c, self.owner, t.id)?.is_none() {
                        if let Some(f) = t.folder_id
                            && store::load_folder(self.c, self.owner, f)?.is_none()
                        {
                            return Err(ApiError::not_found(format!("“{}”原所在的文件夹已删除", t.title.as_str())));
                        }
                        self.create_task(Task { updated_at: self.now, updated_via: self.via.clone(), ..*t })?;
                    }
                }
                Effect::Task(Change::Updated { before: b, after: a }) => {
                    if let Some(cur) = store::load_task(self.c, self.owner, b.id)? {
                        let restored = restore!(cur, b, a, folder_id, title, description, done_at, due, sort_key);
                        self.update_task(cur, restored)?;
                    }
                }
                Effect::Dependency(Link::Added(d)) => {
                    if let Some(d) = store::load_dependency(self.c, self.owner, d.before, d.after)? {
                        self.delete_dependency(d)?;
                    }
                }
                Effect::Dependency(Link::Removed(d)) => {
                    for end in [d.before, d.after] {
                        if store::load_task(self.c, self.owner, end)?.is_none() {
                            return Err(ApiError::not_found("要恢复的依赖所关联的事项已删除"));
                        }
                    }
                    if store::load_dependency(self.c, self.owner, d.before, d.after)?.is_none() {
                        self.create_dependency(d)?;
                    }
                }
            }
        }
        Ok(())
    }
}
