//! What follows from a user's data: task status, the order tasks are listed in and folder paths.
//! The web app has a TypeScript twin of this module; both are checked against `fixtures/board.json`.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{AiJob, Dependency, Folder, FolderId, SortKey, Task, TaskId};

/// All of a user's data.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Snapshot {
    pub folders: Vec<Folder>,
    pub tasks: Vec<Task>,
    pub dependencies: Vec<Dependency>,
    pub ai_jobs: Vec<AiJob>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not done, and every prerequisite is done.
    Ready,
    /// Not done; the number of prerequisites that are not done yet.
    Waiting(usize),
    Done,
}

/// A snapshot indexed for lookups. `None` as a folder means the root.
pub struct Board<'a> {
    tasks: HashMap<TaskId, &'a Task>,
    numbers: HashMap<i64, &'a Task>,
    folders: HashMap<FolderId, &'a Folder>,
    before: HashMap<TaskId, Vec<TaskId>>,
    after: HashMap<TaskId, Vec<TaskId>>,
    /// Subfolders in manual order.
    children: HashMap<Option<FolderId>, Vec<&'a Folder>>,
    contents: HashMap<Option<FolderId>, Vec<&'a Task>>,
    /// Position of each folder when the tree is listed top to bottom, root first.
    rank: HashMap<Option<FolderId>, usize>,
}

impl<'a> Board<'a> {
    pub fn new(s: &'a Snapshot) -> Self {
        let mut children: HashMap<Option<FolderId>, Vec<&Folder>> = HashMap::new();
        for f in &s.folders {
            children.entry(f.parent_id).or_default().push(f);
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| (&a.sort_key, a.id).cmp(&(&b.sort_key, b.id)));
        }
        let mut rank = HashMap::new();
        let mut stack = vec![None];
        while let Some(f) = stack.pop() {
            rank.insert(f, rank.len());
            stack.extend(children.get(&f).into_iter().flatten().rev().map(|c| Some(c.id)));
        }
        let mut contents: HashMap<Option<FolderId>, Vec<&Task>> = HashMap::new();
        for t in &s.tasks {
            contents.entry(t.folder_id).or_default().push(t);
        }
        let mut before: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        let mut after: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        for d in &s.dependencies {
            before.entry(d.after).or_default().push(d.before);
            after.entry(d.before).or_default().push(d.after);
        }
        Board {
            tasks: s.tasks.iter().map(|t| (t.id, t)).collect(),
            numbers: s.tasks.iter().map(|t| (t.number, t)).collect(),
            folders: s.folders.iter().map(|f| (f.id, f)).collect(),
            before,
            after,
            children,
            contents,
            rank,
        }
    }

    pub fn task(&self, id: TaskId) -> Option<&'a Task> {
        self.tasks.get(&id).copied()
    }

    pub fn task_by_number(&self, number: i64) -> Option<&'a Task> {
        self.numbers.get(&number).copied()
    }

    pub fn folder(&self, id: FolderId) -> Option<&'a Folder> {
        self.folders.get(&id).copied()
    }

    /// Prerequisites of a task.
    pub fn before(&self, t: &Task) -> Vec<&'a Task> {
        self.linked(&self.before, t)
    }

    /// Tasks that wait for this one.
    pub fn after(&self, t: &Task) -> Vec<&'a Task> {
        self.linked(&self.after, t)
    }

    fn linked(&self, edges: &HashMap<TaskId, Vec<TaskId>>, t: &Task) -> Vec<&'a Task> {
        let mut list: Vec<&Task> = edges.get(&t.id).into_iter().flatten().map(|id| self.tasks[id]).collect();
        self.sort(&mut list);
        list
    }

    pub fn status(&self, t: &Task) -> Status {
        if t.done_at.is_some() {
            return Status::Done;
        }
        match self.before.get(&t.id).into_iter().flatten().filter(|id| self.tasks[*id].done_at.is_none()).count() {
            0 => Status::Ready,
            n => Status::Waiting(n),
        }
    }

    /// Tasks that become ready once this one is done.
    pub fn unlocks(&self, t: &Task) -> Vec<&'a Task> {
        if t.done_at.is_some() {
            return Vec::new();
        }
        self.after(t).into_iter().filter(|a| a.done_at.is_none() && self.status(a) == Status::Waiting(1)).collect()
    }

    pub fn subfolders(&self, f: Option<FolderId>) -> &[&'a Folder] {
        self.children.get(&f).map_or(&[], Vec::as_slice)
    }

    /// Every folder, root first, each followed by its subfolders.
    pub fn folder_tree(&self) -> Vec<Option<FolderId>> {
        let mut list: Vec<_> = self.rank.keys().copied().collect();
        list.sort_by_key(|f| self.rank[f]);
        list
    }

    /// Whether `f` is `root` or inside it.
    pub fn in_subtree(&self, mut f: Option<FolderId>, root: Option<FolderId>) -> bool {
        loop {
            if f == root {
                return true;
            }
            match f {
                Some(id) => f = self.folders[&id].parent_id,
                None => return false,
            }
        }
    }

    /// `/` for the root, otherwise `/工作/网站改版`.
    pub fn path(&self, f: Option<FolderId>) -> String {
        let mut names = Vec::new();
        let mut cur = f;
        while let Some(id) = cur {
            let folder = self.folders[&id];
            names.push(folder.name.as_str());
            cur = folder.parent_id;
        }
        names.reverse();
        format!("/{}", names.join("/"))
    }

    /// The folder at a path such as `/工作/网站改版` (the leading `/` is optional).
    pub fn find_folder(&self, path: &str) -> Option<Option<FolderId>> {
        let mut cur = None;
        for name in path.split('/').map(str::trim).filter(|s| !s.is_empty()) {
            cur = Some(self.subfolders(cur).iter().find(|f| f.name.as_str() == name)?.id);
        }
        Some(cur)
    }

    /// Tasks directly in a folder: unfinished ones in dependency order, then finished ones, most
    /// recently finished first.
    pub fn tasks_in(&self, f: Option<FolderId>) -> Vec<&'a Task> {
        let (mut done, pending): (Vec<&Task>, Vec<&Task>) = self.contents(f).iter().partition(|t| t.done_at.is_some());
        let mut list = self.order(pending, |t| t.done_at.is_none());
        done.sort_by_key(|t| (Reverse(t.done_at), t.id));
        list.extend(done);
        list
    }

    /// Ready tasks: those with a due date first, earliest first (a date without a time counts as
    /// the end of that day), then the rest in folder order and manual order.
    pub fn next(&self) -> Vec<&'a Task> {
        let mut list: Vec<&Task> = self.tasks.values().copied().filter(|t| self.status(t) == Status::Ready).collect();
        list.sort_by_key(|&t| {
            let due = t.due.as_ref().map(|d| (d.date.as_str(), d.at.as_ref().map_or("24:00", |at| at.time.as_str())));
            (due.is_none(), due, self.key(t))
        });
        list
    }

    /// Sorts tasks in folder order and manual order, ignoring dependencies.
    pub fn sort(&self, list: &mut [&'a Task]) {
        list.sort_by_key(|&t| self.key(t));
    }

    /// Orders tasks so that every task comes after the tasks it depends on, directly or through
    /// other tasks for which `through` holds; otherwise in folder order and manual order. Among the
    /// tasks that can go next, the one earliest in manual order goes first.
    pub fn order(&self, tasks: Vec<&'a Task>, through: impl Fn(&Task) -> bool) -> Vec<&'a Task> {
        let members: HashSet<TaskId> = tasks.iter().map(|t| t.id).collect();
        // The members and the tasks through which they depend on each other.
        let mut relevant = HashSet::new();
        let mut stack: Vec<TaskId> = members.iter().copied().collect();
        while let Some(id) = stack.pop() {
            if relevant.insert(id) {
                stack.extend(self.before.get(&id).into_iter().flatten().filter(|p| members.contains(p) || through(self.tasks[*p])));
            }
        }
        // Kahn's algorithm. The other tasks go as soon as nothing holds them back; members wait
        // in the heap for their turn in manual order.
        let mut waiting: HashMap<TaskId, usize> =
            relevant.iter().map(|id| (*id, self.before.get(id).into_iter().flatten().filter(|p| relevant.contains(*p)).count())).collect();
        let mut free: Vec<TaskId> = waiting.iter().filter(|(_, n)| **n == 0).map(|(id, _)| *id).collect();
        let mut heap = BinaryHeap::new();
        let mut list = Vec::with_capacity(tasks.len());
        loop {
            let id = if let Some(id) = free.pop() {
                if members.contains(&id) {
                    heap.push(Reverse(self.key(self.tasks[&id])));
                    continue;
                }
                id
            } else if let Some(Reverse((_, _, id))) = heap.pop() {
                list.push(self.tasks[&id]);
                id
            } else {
                break;
            };
            for next in self.after.get(&id).into_iter().flatten() {
                if let Some(n) = waiting.get_mut(next) {
                    *n -= 1;
                    if *n == 0 {
                        free.push(*next);
                    }
                }
            }
        }
        list
    }

    /// Tasks directly in a folder, in no particular order.
    pub fn contents(&self, f: Option<FolderId>) -> &[&'a Task] {
        self.contents.get(&f).map_or(&[], Vec::as_slice)
    }

    fn key(&self, t: &'a Task) -> (usize, &'a SortKey, TaskId) {
        (self.rank[&t.folder_id], &t.sort_key, t.id)
    }
}
