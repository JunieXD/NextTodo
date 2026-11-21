//! What follows from a user's data: task status, the order tasks are listed in, the steps along
//! dependencies and folder paths.
//! The web app has a TypeScript twin of this module; both are checked against `fixtures/board.json`.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{
    AiJob, Attachment, Completion, Date, Dependency, Due, Folder, FolderId, Holder, Person, Share, SortKey, Task, TaskId, Time,
};

/// All a user can see: their own data and the folders shared with them, where a folder someone else
/// shares has no parent (docs/design.md#共享).
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Snapshot {
    pub folders: Vec<Folder>,
    pub tasks: Vec<Task>,
    pub dependencies: Vec<Dependency>,
    /// Of the repeating tasks, by task and the day due.
    #[serde(default)]
    pub completions: Vec<Completion>,
    /// Of the tasks and of the user's AI jobs, by id.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    pub ai_jobs: Vec<AiJob>,
    /// The folders shared by the user or with them.
    pub shares: Vec<Share>,
    /// The user, everyone in those folders, and whoever made or last changed a task the user sees.
    pub people: Vec<Person>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not done, every prerequisite done, and the start time come.
    Ready,
    /// Not done: the prerequisites not done yet, and whether the start time is still to come.
    Waiting {
        on: usize,
        not_started: bool,
    },
    Done,
}

/// When a board is as of: the user's today, and the time of day there when known. Without the time,
/// all of today has come: start times that day count as come (docs/design.md#开始时间).
#[derive(Clone, Debug)]
pub struct Now {
    pub today: Date,
    pub time: Option<Time>,
}

impl Now {
    /// The day and time in UTC at `ms` milliseconds since 1970-01-01: now for those who don't say where
    /// they are.
    pub fn utc(ms: i64) -> Now {
        let minutes = ms.rem_euclid(86_400_000) / 60_000;
        let time = Time::try_from(format!("{:02}:{:02}", minutes / 60, minutes % 60)).expect("a time of day");
        Now { today: Date::utc(ms), time: Some(time) }
    }

    /// Whether a start on `date`, at `time` or else at the start of the day, is still to come.
    pub fn before(&self, date: &Date, time: Option<&Time>) -> bool {
        match date.cmp(&self.today) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => time.zip(self.time.as_ref()).is_some_and(|(at, now)| at > now),
        }
    }
}

/// When a repeating task done before holds up the tasks after it again: as its current occurrence
/// begins, at its start or else on its due date (docs/design.md#重复).
fn again(t: &Task) -> Option<(&Date, Option<&Time>)> {
    let due = t.due.as_ref().filter(|d| d.repeat.is_some() && t.done_at.is_none() && t.completions.is_some())?;
    Some(match &t.start {
        Some(start) => (start, t.start_time.as_ref()),
        None => (&due.date, None),
    })
}

/// A snapshot indexed for lookups, as of `now`, for the user `me`. `None` as a folder means the root,
/// the user's own.
pub struct Board<'a> {
    now: Now,
    me: &'a str,
    tasks: HashMap<TaskId, &'a Task>,
    /// By owner and number.
    numbers: HashMap<(&'a str, i64), &'a Task>,
    folders: HashMap<FolderId, &'a Folder>,
    people: HashMap<&'a str, &'a Person>,
    /// The share each folder in a shared folder is in, the shared folder itself too.
    shared: HashMap<FolderId, &'a Share>,
    /// The folders others share with the user, by who shares them and name.
    foreign: Vec<&'a Folder>,
    before: HashMap<TaskId, Vec<TaskId>>,
    after: HashMap<TaskId, Vec<TaskId>>,
    /// Each task's attachments, in the order they were added.
    attached: HashMap<TaskId, Vec<&'a Attachment>>,
    /// Subfolders in manual order.
    children: HashMap<Option<FolderId>, Vec<&'a Folder>>,
    contents: HashMap<Option<FolderId>, Vec<&'a Task>>,
    /// Position of each folder when the tree is listed top to bottom, root first.
    rank: HashMap<Option<FolderId>, usize>,
}

impl<'a> Board<'a> {
    pub fn new(s: &'a Snapshot, now: Now, me: &'a str) -> Self {
        let people: HashMap<&str, &Person> = s.people.iter().map(|p| (p.id.as_str(), p)).collect();
        let shares: HashMap<FolderId, &Share> = s.shares.iter().map(|sh| (sh.folder_id, sh)).collect();
        let mut children: HashMap<Option<FolderId>, Vec<&Folder>> = HashMap::new();
        let mut foreign = Vec::new();
        for f in &s.folders {
            if shares.get(&f.id).is_some_and(|sh| sh.owner != me) {
                foreign.push(f);
            } else {
                children.entry(f.parent_id).or_default().push(f);
            }
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| (&a.sort_key, a.id).cmp(&(&b.sort_key, b.id)));
        }
        let username = |f: &Folder| people.get(shares[&f.id].owner.as_str()).map(|p| p.username.as_str());
        foreign.sort_by(|a, b| (username(a), &a.name, a.id).cmp(&(username(b), &b.name, b.id)));
        let mut rank = HashMap::new();
        let mut shared = HashMap::new();
        let mut stack: Vec<(Option<FolderId>, Option<&Share>)> = foreign.iter().rev().map(|f| (Some(f.id), None)).collect();
        stack.push((None, None));
        while let Some((f, within)) = stack.pop() {
            rank.insert(f, rank.len());
            let within = f.and_then(|id| shares.get(&id).copied()).or(within);
            if let (Some(id), Some(sh)) = (f, within) {
                shared.insert(id, sh);
            }
            stack.extend(children.get(&f).into_iter().flatten().rev().map(|c| (Some(c.id), within)));
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
        let mut attached: HashMap<TaskId, Vec<&Attachment>> = HashMap::new();
        for a in &s.attachments {
            if let Holder::Task(t) = a.holder {
                attached.entry(t).or_default().push(a);
            }
        }
        // Ids are UUIDv7: in the order they were made.
        for list in attached.values_mut() {
            list.sort_by_key(|a| a.id);
        }
        let owner = |t: &Task| t.folder_id.and_then(|f| shared.get(&f)).map_or(me, |sh: &&Share| sh.owner.as_str());
        let numbers = s.tasks.iter().map(|t| ((owner(t), t.number), t)).collect();
        Board {
            now,
            me,
            tasks: s.tasks.iter().map(|t| (t.id, t)).collect(),
            numbers,
            folders: s.folders.iter().map(|f| (f.id, f)).collect(),
            people,
            shared,
            foreign,
            before,
            after,
            attached,
            children,
            contents,
            rank,
        }
    }

    pub fn now(&self) -> &Now {
        &self.now
    }

    pub fn task(&self, id: TaskId) -> Option<&'a Task> {
        self.tasks.get(&id).copied()
    }

    /// Every task the user sees, in no particular order.
    pub fn tasks(&self) -> impl Iterator<Item = &'a Task> + '_ {
        self.tasks.values().copied()
    }

    /// A task's attachments, in the order they were added.
    pub fn attachments(&self, t: &Task) -> &[&'a Attachment] {
        self.attached.get(&t.id).map_or(&[], Vec::as_slice)
    }

    /// The task with `number` among those of `owner`, a user id.
    pub fn task_by_number(&self, owner: &str, number: i64) -> Option<&'a Task> {
        self.numbers.get(&(owner, number)).copied()
    }

    pub fn me(&self) -> &'a str {
        self.me
    }

    pub fn person(&self, id: &str) -> Option<&'a Person> {
        self.people.get(id).copied()
    }

    /// Someone in the user's shared folders, the user too, by the username they sign in with, which
    /// is the same whatever the case.
    pub fn person_named(&self, username: &str) -> Option<&'a Person> {
        self.people.values().copied().find(|p| p.username.eq_ignore_ascii_case(username))
    }

    /// The share a folder is in: its own, or that of the shared folder it is in.
    pub fn share(&self, f: Option<FolderId>) -> Option<&'a Share> {
        self.shared.get(&f?).copied()
    }

    /// Whose what is in folder `f` is: the owner of the shared folder it is in, or the user's.
    pub fn owner_of(&self, f: Option<FolderId>) -> &'a str {
        self.share(f).map_or(self.me, |sh| sh.owner.as_str())
    }

    /// Whose a task is.
    pub fn owner(&self, t: &Task) -> &'a str {
        self.owner_of(t.folder_id)
    }

    /// `#42`, or `@alice#42` for a task in a folder someone else shares.
    pub fn label(&self, t: &Task) -> String {
        match self.owner(t) {
            o if o == self.me => format!("#{}", t.number),
            o => format!("@{}#{}", self.person(o).map_or(o, |p| p.username.as_str()), t.number),
        }
    }

    /// Whether the user is to do it: in a shared folder, if it is assigned to them or to nobody.
    pub fn mine(&self, t: &Task) -> bool {
        self.share(t.folder_id).is_none() || t.assignees.is_empty() || t.assignees.iter().any(|a| a == self.me)
    }

    /// The folders others share with the user.
    pub fn shared_with_me(&self) -> &[&'a Folder] {
        &self.foreign
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
        let on = self.before.get(&t.id).into_iter().flatten().filter(|id| self.holds_up(self.tasks[*id])).count();
        let not_started = t.start.as_ref().is_some_and(|s| self.now.before(s, t.start_time.as_ref()));
        if on == 0 && !not_started { Status::Ready } else { Status::Waiting { on, not_started } }
    }

    /// Whether the tasks after this one wait for it: it is not done, and if it repeats, it has not
    /// been done yet or its current occurrence has begun (docs/design.md#重复).
    pub fn holds_up(&self, t: &Task) -> bool {
        t.done_at.is_none() && again(t).is_none_or(|(date, time)| !self.now.before(date, time))
    }

    /// Whether a repeating task done now would still hold up the tasks after it: its next time has
    /// begun by now too, as one caught up on is due today again.
    fn held_again(&self, t: &Task) -> bool {
        let Some(Due { date, repeat: Some(r), .. }) = &t.due else { return false };
        let Some(next) = r.next(date, &self.now.today) else { return false };
        match &t.start {
            Some(start) => !self.now.before(&start.shifted(date, &next), t.start_time.as_ref()),
            None => !self.now.before(&next, None),
        }
    }

    /// Tasks that become ready once this one is done.
    pub fn unlocks(&self, t: &Task) -> Vec<&'a Task> {
        if !self.holds_up(t) || self.held_again(t) {
            return Vec::new();
        }
        self.after(t).into_iter().filter(|a| self.status(a) == Status::Waiting { on: 1, not_started: false }).collect()
    }

    /// For each task linked to others, which step it is along its dependencies and how many there are
    /// on the longest chain through it (docs/design.md#关联进度). Done tasks are steps too.
    pub fn steps(&self) -> HashMap<TaskId, (usize, usize)> {
        // Kahn's algorithm: each task after its prerequisites.
        let mut waiting: HashMap<TaskId, usize> = self.before.iter().map(|(id, b)| (*id, b.len())).collect();
        let mut order: Vec<TaskId> = self.after.keys().filter(|id| !waiting.contains_key(id)).copied().collect();
        let mut i = 0;
        while let Some(&id) = order.get(i) {
            for next in self.after.get(&id).into_iter().flatten() {
                let n = waiting.get_mut(next).expect("a task with prerequisites");
                *n -= 1;
                if *n == 0 {
                    order.push(*next);
                }
            }
            i += 1;
        }
        let longest = |edges: &HashMap<TaskId, Vec<TaskId>>, counted: &HashMap<TaskId, usize>, id| {
            1 + edges.get(id).into_iter().flatten().map(|other| counted[other]).max().unwrap_or(0)
        };
        let mut depth = HashMap::new();
        for id in &order {
            depth.insert(*id, longest(&self.before, &depth, id));
        }
        let mut height = HashMap::new();
        for id in order.iter().rev() {
            height.insert(*id, longest(&self.after, &height, id));
        }
        order.iter().map(|id| (*id, (depth[id], depth[id] + height[id] - 1))).collect()
    }

    pub fn subfolders(&self, f: Option<FolderId>) -> &[&'a Folder] {
        self.children.get(&f).map_or(&[], Vec::as_slice)
    }

    /// Every folder, root first, each followed by its subfolders, then those shared with the user.
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

    /// `/` for the root, otherwise `/工作/网站改版`; `@alice/装修/水电` in a folder Alice shares.
    pub fn path(&self, f: Option<FolderId>) -> String {
        let mut names = Vec::new();
        let mut cur = f;
        while let Some(id) = cur {
            let folder = self.folders[&id];
            names.push(folder.name.as_str());
            cur = folder.parent_id;
        }
        names.reverse();
        match self.share(f).filter(|sh| sh.owner != self.me) {
            Some(sh) => format!("@{}/{}", self.person(&sh.owner).map_or(sh.owner.as_str(), |p| p.username.as_str()), names.join("/")),
            None => format!("/{}", names.join("/")),
        }
    }

    /// The folder at a path such as `/工作/网站改版` (the leading `/` is optional) or `@alice/装修`.
    pub fn find_folder(&self, path: &str) -> Option<Option<FolderId>> {
        let mut names = path.split('/').map(str::trim).filter(|s| !s.is_empty());
        let mut cur = None;
        if let Some(user) = path.trim_start().strip_prefix('@') {
            let user = user.split('/').next().unwrap_or_default().trim();
            names.next();
            let name = names.next()?;
            let owner = self.person_named(user)?;
            cur = Some(self.foreign.iter().find(|f| f.name.as_str() == name && self.shared[&f.id].owner == owner.id)?.id);
        }
        for name in names {
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

    /// The user's ready tasks, in the order of `by_due`.
    pub fn next(&self) -> Vec<&'a Task> {
        let mut list: Vec<&Task> = self.tasks().filter(|t| self.mine(t) && self.status(t) == Status::Ready).collect();
        self.by_due(&mut list);
        list
    }

    /// Sorts tasks with a due date first, earliest first (a date without a time counts as the end of
    /// that day), then the rest in folder order and manual order.
    pub fn by_due(&self, list: &mut [&'a Task]) {
        list.sort_by_key(|&t| {
            let due = t.due.as_ref().map(|d| (d.date.as_str(), d.at.as_ref().map_or("24:00", |at| at.time.as_str())));
            (due.is_none(), due, self.key(t))
        });
    }

    /// When the first of the user's open tasks still to start does, or a repeating task they wait for
    /// holds them up again: the date, and the time if it has one. Shown with what is ready, so that
    /// what shows it can be fetched again then.
    pub fn next_start(&self) -> Option<(&'a Date, Option<&'a Time>)> {
        let open = || self.tasks.values().copied().filter(|t| t.done_at.is_none() && self.mine(t));
        let starts = open().filter_map(|t| Some((t.start.as_ref()?, t.start_time.as_ref())));
        let again = open().flat_map(|t| self.before.get(&t.id).into_iter().flatten()).filter_map(|id| again(self.tasks[id]));
        starts.chain(again).filter(|(date, time)| self.now.before(date, *time)).min()
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

    /// Tasks in a folder and in its subfolders, in no particular order.
    pub fn within(&self, f: Option<FolderId>) -> Vec<&'a Task> {
        let mut tasks = self.contents(f).to_vec();
        for sub in self.subfolders(f) {
            tasks.extend(self.within(Some(sub.id)));
        }
        tasks
    }

    fn key(&self, t: &'a Task) -> (usize, &'a SortKey, TaskId) {
        (self.rank[&t.folder_id], &t.sort_key, t.id)
    }
}
