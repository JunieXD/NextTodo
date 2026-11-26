//! Where things are and who sees them, and each write to the store with the effect it makes.

use std::collections::BTreeSet;

use nexttodo_core::dag::find_path;
use nexttodo_core::model::{
    AiJob, AiJobId, Attachment, AttachmentId, Change, Completion, Dependency, Effect, Folder, FolderId, Holder, JobKind, Link, Person,
    Share, Task, TaskId,
};

use crate::error::ApiError;
use crate::store;

use super::check::{check_task, due_after};
use super::{Cx, Side, Space};

/// Who sees a share: its owner and members.
fn share_side(s: &Share) -> Side {
    Side::of(std::iter::once(&s.owner).chain(&s.members).cloned().collect())
}

impl Cx<'_> {
    /// The shared folder `id` is in, or is.
    fn root(&self, id: FolderId) -> Result<Option<FolderId>, ApiError> {
        if let Some(root) = self.roots.borrow().get(&id) {
            return Ok(*root);
        }
        let root = store::shared_root(self.c, id)?;
        self.roots.borrow_mut().insert(id, root);
        Ok(root)
    }

    /// Where something of `owner`'s in `folder` is.
    pub(super) fn space(&self, owner: &str, folder: Option<FolderId>) -> Result<Space, ApiError> {
        let root = match folder {
            Some(f) => self.root(f)?,
            None => None,
        };
        let members = match root {
            Some(r) => store::members(self.c, r)?,
            None => Vec::new(),
        };
        Ok(Space { owner: owner.to_owned(), root, members })
    }

    /// A folder the user can see, and its owner.
    pub(super) fn folder(&self, id: FolderId) -> Result<(Folder, String), ApiError> {
        let found = store::load_folder(self.c, id)?;
        match found {
            Some((f, owner)) if owner == self.user || self.space(&owner, Some(id))?.sees(self.user) => Ok((f, owner)),
            _ => Err(ApiError::not_found("文件夹不存在")),
        }
    }

    /// A task the user can see, and its owner.
    pub(super) fn task(&self, id: TaskId) -> Result<(Task, String), ApiError> {
        let found = store::load_task(self.c, id)?;
        match found {
            Some((t, owner)) if owner == self.user || self.space(&owner, t.folder_id)?.sees(self.user) => Ok((t, owner)),
            _ => Err(ApiError::not_found("事项不存在")),
        }
    }

    pub(super) fn job(&self, id: AiJobId) -> Result<AiJob, ApiError> {
        store::load_job(self.c, self.user, id)?.ok_or_else(|| ApiError::not_found("AI 整理请求不存在"))
    }

    /// Who sees what an attachment belongs to, which the user has to see: a task, or their own job.
    pub(super) fn holder_side(&self, holder: Holder) -> Result<Side, ApiError> {
        match holder {
            Holder::Task(t) => {
                let (t, owner) = self.task(t)?;
                Ok(self.space(&owner, t.folder_id)?.side())
            }
            Holder::Job(j) => {
                self.job(j)?;
                Ok(Side::of(vec![self.user.to_owned()]))
            }
        }
    }

    /// An attachment the user can see, and who sees it.
    pub(super) fn attachment(&self, id: AttachmentId) -> Result<(Attachment, Side), ApiError> {
        self.visible_attachment(id)?.ok_or_else(|| ApiError::not_found("附件不存在"))
    }

    pub(super) fn visible_attachment(&self, id: AttachmentId) -> Result<Option<(Attachment, Side)>, ApiError> {
        let Some(a) = store::load_attachment(self.c, id)? else { return Ok(None) };
        match self.holder_side(a.holder) {
            Ok(side) => Ok(Some((a, side))),
            Err(e) if e.code() == "not_found" => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Whose what is put in `folder` is: the folder's owner's, or at the root the user's.
    pub(super) fn owner_in(&self, folder: Option<FolderId>) -> Result<String, ApiError> {
        match folder {
            Some(f) => Ok(self.folder(f)?.1),
            None => Ok(self.user.to_owned()),
        }
    }

    /// Who sees a folder, it being in `space`.
    fn folder_side(f: &Folder, space: &Space) -> Side {
        Side { users: space.people(), root_of: (space.root == Some(f.id)).then(|| space.owner.clone()) }
    }

    fn check_name(&self, owner: &str, f: &Folder) -> Result<(), ApiError> {
        if store::folder_name_taken(self.c, owner, f)? {
            return Err(ApiError::conflict(format!("同一层级已有名为“{}”的文件夹", f.name.as_str())));
        }
        Ok(())
    }

    pub(super) fn create_folder(&mut self, f: Folder, owner: &str) -> Result<(), ApiError> {
        self.check_name(owner, &f)?;
        let space = self.space(owner, f.parent_id)?;
        store::insert_folder(self.c, owner, &f)?;
        self.roots.borrow_mut().insert(f.id, space.root);
        self.push(Effect::Folder(Change::Created(f)), Side::default(), space.side(), false);
        Ok(())
    }

    /// Changes a folder. One moved into or out of a shared folder takes along everything in it; its
    /// tasks lose the dependencies on what stays behind, and the assignees who can't see them now.
    pub(super) fn update_folder(&mut self, before: Folder, owner: &str, mut after: Folder) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        let from = self.space(owner, Some(before.id))?;
        let shared = from.root == Some(before.id);
        if shared && owner != self.user {
            return Err(ApiError::forbidden("只有共享人能改名、移动或删除共享的文件夹"));
        }
        let (new_owner, to) = if after.parent_id == before.parent_id {
            (owner.to_owned(), from.clone())
        } else {
            let new_owner = self.owner_in(after.parent_id)?;
            if let Some(p) = after.parent_id
                && store::in_subtree(self.c, after.id, p)?
            {
                return Err(ApiError::folder_cycle());
            }
            let there = self.space(&new_owner, after.parent_id)?;
            if there.shared() && !store::shared_within(self.c, before.id)?.is_empty() {
                return Err(ApiError::invalid("共享的文件夹不能放进另一个共享的文件夹", None));
            }
            (new_owner, if shared { from.clone() } else { there })
        };
        if after.parent_id != before.parent_id || after.name != before.name {
            self.check_name(&new_owner, &after)?;
        }
        if shared && after.name != before.name && store::shared_name_taken(self.c, owner, after.name.as_str(), after.id)? {
            return Err(ApiError::conflict(format!("已共享过名为“{}”的文件夹", after.name.as_str())));
        }
        let before_side = Self::folder_side(&before, &from);
        let across = to != from;
        if across {
            self.cut_off(before.id, &from)?;
        }
        after.updated_at = self.now;
        store::update_folder(self.c, &after)?;
        if new_owner != owner {
            store::give_subtree(self.c, after.id, &new_owner)?;
        }
        self.roots.borrow_mut().clear();
        if across {
            self.carry(after.id, &from, &to)?;
        }
        let after_side = Self::folder_side(&after, &to);
        self.push(Effect::Folder(Change::Updated { before, after }), before_side, after_side, false);
        Ok(())
    }

    /// Removes the dependencies between the tasks in the subtree rooted at `id` and those outside it.
    fn cut_off(&mut self, id: FolderId, space: &Space) -> Result<(), ApiError> {
        let inside: BTreeSet<TaskId> = store::subtree_tasks(self.c, id)?.into_iter().map(|t| t.id).collect();
        for d in store::subtree_dependencies(self.c, id)? {
            if !(inside.contains(&d.before) && inside.contains(&d.after)) {
                self.delete_dependency(d, space)?;
            }
        }
        Ok(())
    }

    /// Makes what is in the subtree rooted at `id`, moved from `from` to `to`, belong there: its
    /// tasks to `to`'s owner, numbered anew if that is someone else, assigned only to people there;
    /// AI jobs of people who no longer see their folder go to their root, or go, if tidying it. The
    /// folders and the dependencies inside stay as they are, for those who now see them or don't.
    fn carry(&mut self, id: FolderId, from: &Space, to: &Space) -> Result<(), ApiError> {
        for t in store::subtree_tasks(self.c, id)? {
            let mut after = t.clone();
            after.assignees.retain(|u| to.shared() && to.sees(u));
            // Its new number may be the one it had: it is someone else's still.
            let kept = after == t && to.owner == from.owner;
            if to.owner != from.owner {
                after.number = self.number(&to.owner, t.id, self.numbers.get(&t.id).copied())?;
            }
            if !kept {
                store::update_task(self.c, &to.owner, &after)?;
            }
            self.push(Effect::Task(Change::Updated { before: Box::new(t), after: Box::new(after) }), from.side(), to.side(), kept);
        }
        for a in store::subtree_attachments(self.c, id)? {
            self.push(Effect::Attachment(Change::Updated { before: a.clone(), after: a }), from.side(), to.side(), true);
        }
        for (j, owner) in store::subtree_jobs(self.c, id)? {
            if !to.sees(&owner) {
                self.drop_job(j, &owner)?;
            }
        }
        for f in store::subtree_folders(self.c, id)? {
            if f.id != id {
                self.push(Effect::Folder(Change::Updated { before: f.clone(), after: f }), from.side(), to.side(), true);
            }
        }
        for d in store::subtree_dependencies(self.c, id)? {
            self.push(Effect::Dependency(Link::Added(d)), from.side(), to.side(), true);
        }
        for r in store::subtree_completions(self.c, id)? {
            self.push(Effect::Completion(Link::Added(r)), from.side(), to.side(), true);
        }
        Ok(())
    }

    /// Takes a job of `owner`'s out of a folder they no longer see: organizing goes on at their root,
    /// tidying it is over.
    fn drop_job(&mut self, j: AiJob, owner: &str) -> Result<(), ApiError> {
        match j.kind {
            JobKind::Organize => self.update_job(j.clone(), AiJob { folder_id: None, ..j }, owner),
            JobKind::Tidy => self.delete_job(j, owner),
        }
    }

    /// Records the removal of everything in the subtree (dependencies, attachments, completion records,
    /// tasks, AI jobs, folders deepest first)
    /// so that undo can bring it all back in reverse order. Folders are deleted one by one, deepest
    /// first, because a single cascade through a deep tree would hit SQLite's trigger depth limit.
    /// Shared folders inside go first, as their members see them. A shared folder stops being
    /// shared first, its tasks assigned to nobody before, so that bringing it back shares it again
    /// and then assigns them again.
    pub(super) fn delete_folder(&mut self, id: FolderId, owner: &str) -> Result<(), ApiError> {
        let space = self.space(owner, Some(id))?;
        if space.root == Some(id) {
            if owner != self.user {
                return Err(ApiError::forbidden("只有共享人能改名、移动或删除共享的文件夹"));
            }
            for t in store::subtree_tasks(self.c, id)?.into_iter().filter(|t| !t.assignees.is_empty()) {
                let after = Task { assignees: Vec::new(), ..t.clone() };
                store::update_task(self.c, owner, &after)?;
                self.push(Effect::Task(Change::Updated { before: Box::new(t), after: Box::new(after) }), space.side(), space.side(), false);
            }
            let share = Share { folder_id: id, owner: owner.to_owned(), members: space.members.clone() };
            self.push(Effect::Share(Change::Deleted(share)), space.side(), Side::default(), false);
        } else if space.root.is_none() {
            for shared in store::shared_within(self.c, id)? {
                self.delete_folder(shared, owner)?;
            }
        }
        for d in store::subtree_dependencies(self.c, id)? {
            self.push(Effect::Dependency(Link::Removed(d)), space.side(), Side::default(), false);
        }
        for a in store::subtree_attachments(self.c, id)? {
            self.delete_attachment(a, space.side())?;
        }
        for r in store::subtree_completions(self.c, id)? {
            self.push(Effect::Completion(Link::Removed(r)), space.side(), Side::default(), false);
        }
        for t in store::subtree_tasks(self.c, id)? {
            self.push(Effect::Task(Change::Deleted(Box::new(t))), space.side(), Side::default(), false);
        }
        for (j, owner) in store::subtree_jobs(self.c, id)? {
            self.delete_job(j, &owner)?;
        }
        for f in store::subtree_folders(self.c, id)? {
            store::delete_folder(self.c, f.id)?;
            let side = Self::folder_side(&f, &space);
            self.push(Effect::Folder(Change::Deleted(f)), side, Side::default(), false);
        }
        self.roots.borrow_mut().clear();
        Ok(())
    }

    pub(super) fn create_task(&mut self, mut t: Task, owner: &str) -> Result<(), ApiError> {
        t.due = due_after(&t).unwrap_or(t.due);
        check_task(&t)?;
        let space = self.space(owner, t.folder_id)?;
        t.assignees.retain(|u| space.shared() && space.sees(u));
        store::insert_task(self.c, owner, &t)?;
        self.push(Effect::Task(Change::Created(Box::new(t))), Side::default(), space.side(), false);
        Ok(())
    }

    /// Changes a task. One moved into or out of a shared folder loses its dependencies, which were
    /// all on what stays behind, and the assignees who can't see it now, takes its attachments along
    /// for who sees it now, and belongs to the owner of
    /// where it is: numbered anew if that is someone else, or with the number it had there, when that
    /// is `had`, undoing such a move, and still free.
    pub(super) fn update_task(&mut self, before: Task, owner: &str, mut after: Task, had: Option<i64>) -> Result<(), ApiError> {
        after.due = due_after(&after).unwrap_or(after.due);
        if after == before {
            return Ok(());
        }
        check_task(&after)?;
        let from = self.space(owner, before.folder_id)?;
        let (new_owner, to) = if after.folder_id == before.folder_id {
            (owner.to_owned(), from.clone())
        } else {
            let new_owner = self.owner_in(after.folder_id)?;
            let to = self.space(&new_owner, after.folder_id)?;
            (new_owner, to)
        };
        if to != from {
            for d in store::task_dependencies(self.c, before.id)? {
                self.delete_dependency(d, &from)?;
            }
        }
        after.assignees.retain(|u| to.shared() && to.sees(u));
        if new_owner != owner {
            after.number = self.number(&new_owner, after.id, had)?;
        }
        after.updated_at = self.now;
        after.updated_by = self.user.to_owned();
        after.updated_via = self.via.clone();
        store::update_task(self.c, &new_owner, &after)?;
        let id = after.id;
        self.push(Effect::Task(Change::Updated { before: Box::new(before), after: Box::new(after) }), from.side(), to.side(), false);
        if to != from {
            for a in store::task_attachments(self.c, id)? {
                self.push(Effect::Attachment(Change::Updated { before: a.clone(), after: a }), from.side(), to.side(), true);
            }
            for r in store::task_completions(self.c, id)? {
                self.push(Effect::Completion(Link::Added(r)), from.side(), to.side(), true);
            }
        }
        Ok(())
    }

    /// The number task `id` gets as it goes to `owner`: `had`, the one it had there, undoing the
    /// change that took it away, when `owner` still has it free; otherwise the next one.
    pub(super) fn number(&self, owner: &str, id: TaskId, had: Option<i64>) -> Result<i64, ApiError> {
        match had {
            Some(n) if store::number_free(self.c, owner, n, id)? => Ok(n),
            _ => Ok(store::take_task_number(self.c, owner)?),
        }
    }

    pub(super) fn delete_task(&mut self, id: TaskId) -> Result<(), ApiError> {
        let (t, owner) = self.task(id)?;
        let space = self.space(&owner, t.folder_id)?;
        for d in store::task_dependencies(self.c, id)? {
            self.push(Effect::Dependency(Link::Removed(d)), space.side(), Side::default(), false);
        }
        for a in store::task_attachments(self.c, id)? {
            self.delete_attachment(a, space.side())?;
        }
        for r in store::task_completions(self.c, id)? {
            self.push(Effect::Completion(Link::Removed(r)), space.side(), Side::default(), false);
        }
        store::delete_task(self.c, id)?;
        self.push(Effect::Task(Change::Deleted(Box::new(t))), space.side(), Side::default(), false);
        Ok(())
    }

    pub(super) fn create_job(&mut self, j: AiJob) -> Result<(), ApiError> {
        store::insert_job(self.c, self.user, &j)?;
        let side = Side::of(vec![self.user.to_owned()]);
        self.push(Effect::AiJob(Change::Created(Box::new(j))), Side::default(), side, false);
        Ok(())
    }

    /// Changes a job of `owner`'s.
    pub(super) fn update_job(&mut self, before: AiJob, mut after: AiJob, owner: &str) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        after.updated_at = self.now;
        store::update_job(self.c, &after)?;
        let side = Side::of(vec![owner.to_owned()]);
        self.push(Effect::AiJob(Change::Updated { before: Box::new(before), after: Box::new(after) }), side.clone(), side, false);
        Ok(())
    }

    pub(super) fn delete_job(&mut self, j: AiJob, owner: &str) -> Result<(), ApiError> {
        for a in store::job_attachments(self.c, j.id)? {
            self.delete_attachment(a, Side::of(vec![owner.to_owned()]))?;
        }
        store::delete_job(self.c, j.id)?;
        self.push(Effect::AiJob(Change::Deleted(Box::new(j))), Side::of(vec![owner.to_owned()]), Side::default(), false);
        Ok(())
    }

    pub(super) fn create_attachment(&mut self, a: Attachment, side: Side) -> Result<(), ApiError> {
        store::insert_attachment(self.c, &a)?;
        self.push(Effect::Attachment(Change::Created(a)), Side::default(), side, false);
        Ok(())
    }

    /// Renames an attachment, or moves it to what the user sees too.
    pub(super) fn update_attachment(&mut self, before: Attachment, side: Side, after: Attachment) -> Result<(), ApiError> {
        if after == before {
            return Ok(());
        }
        let after_side = if after.holder == before.holder { side.clone() } else { self.holder_side(after.holder)? };
        store::update_attachment(self.c, &after)?;
        self.push(Effect::Attachment(Change::Updated { before, after }), side, after_side, false);
        Ok(())
    }

    pub(super) fn delete_attachment(&mut self, a: Attachment, side: Side) -> Result<(), ApiError> {
        store::delete_attachment(self.c, a.id, self.now)?;
        self.push(Effect::Attachment(Change::Deleted(a)), side, Side::default(), false);
        Ok(())
    }

    pub(super) fn create_dependency(&mut self, d: Dependency, space: &Space) -> Result<(), ApiError> {
        let edges = store::dependency_edges(self.c, &space.owner)?;
        if let Some(path) = find_path(&edges, d.after, d.before) {
            let path: Vec<TaskId> = [d.before].into_iter().chain(path).collect();
            let titles =
                path.iter().map(|&id| Ok(format!("“{}”", self.task(id)?.0.title.as_str()))).collect::<Result<Vec<_>, ApiError>>()?;
            return Err(ApiError::cycle(path, &titles.join(" → ")));
        }
        store::insert_dependency(self.c, &space.owner, &d)?;
        self.push(Effect::Dependency(Link::Added(d)), Side::default(), space.side(), false);
        Ok(())
    }

    pub(super) fn delete_dependency(&mut self, d: Dependency, space: &Space) -> Result<(), ApiError> {
        store::delete_dependency(self.c, &d)?;
        self.push(Effect::Dependency(Link::Removed(d)), space.side(), Side::default(), false);
        Ok(())
    }

    /// Records a time its task was done.
    pub(super) fn record_completion(&mut self, r: Completion) -> Result<(), ApiError> {
        let (t, owner) = self.task(r.task_id)?;
        let space = self.space(&owner, t.folder_id)?;
        store::insert_completion(self.c, &r)?;
        self.push(Effect::Completion(Link::Added(r)), Side::default(), space.side(), false);
        Ok(())
    }

    pub(super) fn delete_completion(&mut self, r: Completion, space: &Space) -> Result<(), ApiError> {
        store::delete_completion(self.c, &r)?;
        self.push(Effect::Completion(Link::Removed(r)), space.side(), Side::default(), false);
        Ok(())
    }

    /// Shares folder `f` of `owner`'s with `person`. The first time, its tasks lose their
    /// dependencies on those outside it, as only what is in it is shared.
    pub(super) fn add_member(&mut self, f: &Folder, owner: &str, person: Person) -> Result<(), ApiError> {
        let space = self.space(owner, Some(f.id))?;
        if space.root.is_some_and(|r| r != f.id) {
            return Err(ApiError::invalid("共享的文件夹里的文件夹不能再共享", None));
        }
        if !space.shared() && !store::shared_within(self.c, f.id)?.is_empty() {
            return Err(ApiError::invalid(format!("“{}”里已有共享的文件夹，不能再共享", f.name.as_str()), None));
        }
        if space.members.contains(&person.id) {
            return Err(ApiError::conflict(format!("{} 已是成员", person.display_name)));
        }
        if !space.shared() {
            if store::shared_name_taken(self.c, owner, f.name.as_str(), f.id)? {
                return Err(ApiError::conflict(format!("已共享过名为“{}”的文件夹，请先改名", f.name.as_str())));
            }
            self.cut_off(f.id, &space)?;
        }
        store::insert_member(self.c, f.id, &person.id)?;
        self.roots.borrow_mut().clear();
        let mut members = space.members.clone();
        members.push(person.id.clone());
        members.sort();
        let before = Share { folder_id: f.id, owner: owner.to_owned(), members: space.members.clone() };
        let after = Share { members, ..before.clone() };
        let side = share_side(&after);
        self.done.resync.push(person.id.clone());
        self.push(Effect::Person(Change::Created(person)), Side::default(), side.clone(), false);
        let change = if space.shared() { Change::Updated { before, after } } else { Change::Created(after) };
        self.push(Effect::Share(change), space.side(), side, false);
        Ok(())
    }

    /// Takes `user` off folder `f`, shared by `owner`: off the tasks they were to do, and their AI
    /// jobs out of it. Once nobody is left, it is no longer shared, and nobody is assigned anything.
    pub(super) fn remove_member(&mut self, f: &Folder, owner: &str, user: &str) -> Result<(), ApiError> {
        let space = self.space(owner, Some(f.id))?;
        if space.root != Some(f.id) || !space.members.iter().any(|m| m == user) {
            return Err(ApiError::not_found("不是这个文件夹的成员"));
        }
        let last = space.members.len() == 1;
        for t in store::subtree_tasks(self.c, f.id)? {
            let mut after = t.clone();
            after.assignees.retain(|u| !last && u != user);
            if after != t {
                store::update_task(self.c, owner, &after)?;
                self.push(Effect::Task(Change::Updated { before: Box::new(t), after: Box::new(after) }), space.side(), space.side(), false);
            }
        }
        for (j, by) in store::subtree_jobs(self.c, f.id)? {
            if by == user {
                self.drop_job(j, &by)?;
            }
        }
        store::delete_member(self.c, f.id, user)?;
        self.roots.borrow_mut().clear();
        let before = Share { folder_id: f.id, owner: owner.to_owned(), members: space.members.clone() };
        let after = Share { members: before.members.iter().filter(|m| *m != user).cloned().collect(), ..before.clone() };
        self.done.resync.push(user.to_owned());
        let change = if last { Change::Deleted(before) } else { Change::Updated { before, after: after.clone() } };
        self.push(Effect::Share(change), space.side(), share_side(&after), false);
        Ok(())
    }
}
