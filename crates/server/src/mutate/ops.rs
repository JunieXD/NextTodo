//! Each operation, as the writes it is made of.

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use nexttodo_core::model::{
    AiJob, Attachment, Completion, Completions, Dependency, Folder, Holder, JobKind, JobState, JobText, SortKey, Task,
};
use nexttodo_core::ops::Op;

use crate::error::ApiError;
use crate::store;

use super::Cx;
use super::check::{at_times, both_due, by_start, check_assignees};
impl Cx<'_> {
    pub(super) fn op(&mut self, op: &Op) -> Result<(), ApiError> {
        match op {
            Op::FolderCreate { id, name, parent_id, sort_key, color, icon } => {
                if store::folder_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("文件夹 ID 已存在"));
                }
                let owner = self.owner_in(*parent_id)?;
                let sort_key = match sort_key {
                    Some(k) => k.clone(),
                    None => SortKey::after(store::last_folder_key(self.c, &owner, *parent_id)?.as_ref()),
                };
                let f = Folder {
                    id: *id,
                    parent_id: *parent_id,
                    name: name.clone(),
                    sort_key,
                    color: *color,
                    icon: *icon,
                    created_at: self.now,
                    updated_at: self.now,
                };
                self.create_folder(f, &owner)
            }
            Op::FolderUpdate { id, name, parent_id, sort_key, color, icon } => {
                let (before, owner) = self.folder(*id)?;
                let mut f = before.clone();
                if let Some(name) = name {
                    f.name = name.clone();
                }
                if let Some(color) = color {
                    f.color = *color;
                }
                if let Some(icon) = icon {
                    f.icon = *icon;
                }
                if let Some(parent_id) = parent_id {
                    f.parent_id = *parent_id;
                }
                if let Some(k) = sort_key {
                    f.sort_key = k.clone();
                }
                self.update_folder(before, &owner, f)
            }
            Op::FolderDelete { id } => {
                let (_, owner) = self.folder(*id)?;
                self.delete_folder(*id, &owner)
            }
            Op::TaskCreate {
                id,
                title,
                folder_id,
                description,
                start,
                start_time,
                start_reminders,
                remind_at,
                due,
                due_after,
                sort_key,
                done,
                assignees,
            } => {
                if store::task_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("事项 ID 已存在"));
                }
                if due.is_some() && due_after.is_some() {
                    return Err(both_due());
                }
                let owner = self.owner_in(*folder_id)?;
                let mut assignees = assignees.clone();
                assignees.sort();
                assignees.dedup();
                check_assignees(&self.space(&owner, *folder_id)?, &assignees)?;
                let sort_key = match sort_key {
                    Some(k) => k.clone(),
                    None => SortKey::after(store::last_task_key(self.c, &owner, *folder_id)?.as_ref()),
                };
                let task = Task {
                    id: *id,
                    number: store::take_task_number(self.c, &owner)?,
                    folder_id: *folder_id,
                    title: title.clone(),
                    description: description.clone(),
                    done_at: done.then_some(self.now),
                    dropped: false,
                    start: start.clone(),
                    start_time: start_time.clone(),
                    start_reminders: by_start(start_time.as_ref(), start_reminders)?,
                    remind_at: at_times(remind_at),
                    due_after: due_after.clone(),
                    unlocked_on: None,
                    due: due.clone(),
                    completions: None,
                    sort_key,
                    assignees,
                    created_at: self.now,
                    updated_at: self.now,
                    created_by: self.user.to_owned(),
                    updated_by: self.user.to_owned(),
                    created_via: self.via.clone(),
                    updated_via: self.via.clone(),
                };
                self.create_task(task, &owner)
            }
            Op::TaskUpdate {
                id,
                folder_id,
                title,
                description,
                start,
                start_time,
                start_reminders,
                remind_at,
                due,
                due_after,
                sort_key,
                done,
                today,
                occurrence,
                assign,
                unassign,
            } => {
                let (before, owner) = self.task(*id)?;
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
                if let Some(start) = start {
                    // The time goes with the date; a new date keeps it.
                    t.start_time = t.start_time.take().filter(|_| start.is_some());
                    t.start = start.clone();
                }
                if let Some(time) = start_time {
                    t.start_time = time.clone();
                }
                match start_reminders {
                    Some(given) => t.start_reminders = by_start(t.start_time.as_ref(), given)?,
                    None => t.fit_start_reminders(),
                }
                if let Some(at) = remind_at {
                    t.remind_at = at_times(at);
                }
                match (due, due_after) {
                    (Some(_), Some(_)) => return Err(both_due()),
                    (Some(due), None) => {
                        t.due = due.clone();
                        t.due_after = None;
                        t.unlocked_on = None;
                    }
                    (None, Some(due_after)) => {
                        // Turned to or from counting after the prerequisites, the due date goes: given
                        // again when they are done, or now if they are. A due date of its own stays when
                        // there was no `due_after` to clear.
                        if t.due_after.is_some() != due_after.is_some() {
                            t.unlocked_on = None;
                            t.due = None;
                        }
                        t.due_after = due_after.clone();
                    }
                    (None, None) => {}
                }
                if let Some(k) = sort_key {
                    t.sort_key = k.clone();
                }
                if !assign.is_empty() || !unassign.is_empty() {
                    let target = if t.folder_id == before.folder_id { owner.clone() } else { self.owner_in(t.folder_id)? };
                    check_assignees(&self.space(&target, t.folder_id)?, assign)?;
                    let mut people: BTreeSet<String> = t.assignees.iter().chain(assign).cloned().collect();
                    people.retain(|u| !unassign.contains(u));
                    t.assignees = people.into_iter().collect();
                }
                let mut record = None;
                // Done from a list behind the task: that time of a repeating one is done already, or
                // gone (docs/api.md#改动).
                let behind = occurrence.as_ref().is_some_and(|o| t.occurrence().is_some_and(|now| now != *o));
                // Done once given up, it was done after all, ending when it did; not a time of a
                // repeating one a list behind showed, which giving it up ended (docs/api.md#改动).
                if *done == Some(true) && t.dropped && occurrence.is_none() {
                    t.dropped = false;
                } else if let Some(done) = done
                    && *done != t.done_at.is_some()
                    && !(*done && behind)
                {
                    t.dropped = false;
                    match t.due.as_mut().map(|d| (d.repeat.clone(), d)) {
                        // Done, a repeating task stays open, due next time; done the last time, it is
                        // done (docs/design.md#重复).
                        Some((Some(r), d)) if *done => {
                            let today = today
                                .as_ref()
                                .ok_or_else(|| ApiError::invalid("完成重复的事项需要给出今天的日期", Some("today".into())))?;
                            let again = store::next_again(self.c, t.id, &d.date)?;
                            record = Some(Completion { task_id: t.id, due: d.date.clone(), done: today.clone(), again });
                            match r.next(&d.date, today) {
                                Some(next) => {
                                    // The start date keeps as far before it, at the same time (docs/design.md#开始时间).
                                    t.start = t.start.as_ref().map(|s| s.shifted(&d.date, &next));
                                    d.date = next;
                                }
                                None => {
                                    d.repeat = None;
                                    t.done_at = Some(self.now);
                                }
                            }
                            let count = t.completions.as_ref().map_or(NonZeroU32::MIN, |c| c.count.saturating_add(1));
                            t.completions = Some(Completions { count, last: today.clone() });
                        }
                        _ => t.done_at = done.then_some(self.now),
                    }
                }
                self.update_task(before, &owner, t, None)?;
                match record {
                    Some(r) => self.record_completion(r),
                    None => Ok(()),
                }
            }
            Op::TaskDrop { id } => {
                let (before, owner) = self.task(*id)?;
                let mut t = before.clone();
                // Done already, it ends given up as it did.
                t.done_at = t.done_at.or(Some(self.now));
                t.dropped = true;
                // Given up for good, it has no next time (docs/design.md#放弃).
                if let Some(d) = &mut t.due {
                    d.repeat = None;
                }
                self.update_task(before, &owner, t, None)
            }
            Op::TaskDelete { id } => {
                self.task(*id)?;
                self.delete_task(*id)
            }
            Op::DepAdd { before, after } => {
                if before == after {
                    return Err(ApiError::invalid("事项不能依赖自己", None));
                }
                let (b, b_owner) = self.task(*before)?;
                let (a, a_owner) = self.task(*after)?;
                let space = self.space(&b_owner, b.folder_id)?;
                if space != self.space(&a_owner, a.folder_id)? {
                    return Err(ApiError::invalid("只能连同一处的事项：私人的事项之间，或同一个共享的文件夹里的事项之间", None));
                }
                if store::load_dependency(self.c, *before, *after)?.is_some() {
                    return Err(ApiError::conflict("该依赖已存在"));
                }
                self.create_dependency(Dependency { before: *before, after: *after, created_at: self.now }, &space)
            }
            Op::DepRemove { before, after } => {
                let (b, owner) = self.task(*before)?;
                let d = store::load_dependency(self.c, *before, *after)?.ok_or_else(|| ApiError::not_found("该依赖不存在"))?;
                let space = self.space(&owner, b.folder_id)?;
                self.delete_dependency(d, &space)
            }
            Op::AttachmentAdd { id, name, media_type, size, task_id, job_id } => {
                let holder = match (task_id, job_id) {
                    (Some(t), None) => Holder::Task(*t),
                    (None, Some(j)) => Holder::Job(*j),
                    _ => return Err(ApiError::invalid("附件要属于一个事项或一条 AI 整理请求：task_id 和 job_id 给且只给一个", None)),
                };
                let side = self.holder_side(holder)?;
                match store::load_file(self.c, *id)? {
                    Some(f) if f.owner == self.user => {
                        if store::attachment_id_taken(self.c, *id)? {
                            return Err(ApiError::conflict("附件 ID 已存在"));
                        }
                        if f.media_type != *media_type || f.size != *size {
                            return Err(ApiError::invalid("附件的类型或大小和上传的文件不一致", None));
                        }
                    }
                    _ => return Err(ApiError::file_missing()),
                }
                let a =
                    Attachment { id: *id, holder, name: name.clone(), media_type: media_type.clone(), size: *size, created_at: self.now };
                self.create_attachment(a, side)
            }
            Op::AttachmentUpdate { id, name, task_id } => {
                let (before, side) = self.attachment(*id)?;
                let mut a = before.clone();
                if let Some(name) = name {
                    a.name = name.clone();
                }
                if let Some(t) = task_id {
                    a.holder = Holder::Task(*t);
                }
                self.update_attachment(before, side, a)
            }
            Op::AttachmentDelete { id } => {
                let (a, side) = self.attachment(*id)?;
                self.delete_attachment(a, side)
            }
            Op::AiJobCreate { id, kind, text, local_time, folder_id, reminders } => {
                if store::job_id_taken(self.c, *id)? {
                    return Err(ApiError::conflict("请求 ID 已存在"));
                }
                asks(kind.unwrap_or_default(), text)?;
                if let Some(f) = folder_id {
                    self.folder(*f)?;
                }
                self.create_job(AiJob {
                    id: *id,
                    folder_id: *folder_id,
                    kind: kind.unwrap_or_default(),
                    text: text.clone(),
                    local_time: local_time.clone(),
                    reminders: reminders.clone(),
                    state: JobState::Waiting,
                    error: None,
                    suggestions: None,
                    created_at: self.now,
                    updated_at: self.now,
                })
            }
            Op::AiJobRetry { id, text, folder_id, reminders } => {
                let before = self.job(*id)?;
                match before.state {
                    JobState::Failed | JobState::Ready => {}
                    JobState::Running => return Err(ApiError::conflict("正在整理，稍后再改")),
                    JobState::Waiting => return Err(ApiError::conflict("这条请求已在等待整理")),
                }
                let mut j =
                    AiJob { state: JobState::Waiting, error: None, suggestions: None, reminders: reminders.clone(), ..before.clone() };
                if let Some(text) = text {
                    asks(j.kind, text)?;
                    j.text = text.clone();
                }
                if let Some(folder_id) = folder_id {
                    if let Some(f) = folder_id {
                        self.folder(*f)?;
                    }
                    j.folder_id = *folder_id;
                }
                let user = self.user;
                self.update_job(before, j, user)
            }
            Op::AiJobDelete { id } => {
                let j = self.job(*id)?;
                let user = self.user;
                self.delete_job(j, user)
            }
            Op::ShareAdd { folder_id, username } => {
                let (f, owner) = self.folder(*folder_id)?;
                if owner != self.user {
                    return Err(ApiError::forbidden("只有共享人能添加成员"));
                }
                let person =
                    store::person_named(self.c, username)?.ok_or_else(|| ApiError::not_found(format!("没有用户“{}”", username.trim())))?;
                if person.id == owner {
                    return Err(ApiError::invalid("不能共享给自己", Some("username".into())));
                }
                self.add_member(&f, &owner, person)
            }
            Op::ShareRemove { folder_id, user_id } => {
                let (f, owner) = self.folder(*folder_id)?;
                if self.user != owner && self.user != user_id {
                    return Err(ApiError::forbidden("只有共享人能移除成员"));
                }
                self.remove_member(&f, &owner, user_id)
            }
            Op::Undo { mutation } => self.undo(*mutation),
        }
    }
}

/// Whether a job of `kind` says what to do with `text`: a tidying has to, an organizing may leave it
/// to its pictures, which only running it can tell the model sees.
fn asks(kind: JobKind, text: &JobText) -> Result<(), ApiError> {
    if kind == JobKind::Tidy && text.as_str().trim().is_empty() {
        return Err(ApiError::invalid("要求不能为空", Some("text".into())));
    }
    Ok(())
}
