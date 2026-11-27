//! Rules a task and its people are checked against, as an operation leaves them.

use nexttodo_core::model::{Due, DueAfter, MAX_REMINDERS, Remind, RemindAt, Task};

use crate::error::ApiError;

use super::Space;

/// Reminders by the start as given, for a start at `time`.
pub(super) fn by_start(time: Option<&nexttodo_core::model::Time>, given: &[Remind]) -> Result<Vec<Remind>, ApiError> {
    nexttodo_core::model::start_reminders(time, given.to_vec()).map_err(|m| ApiError::invalid(m, Some("start_reminders".into())))
}

/// Reminders at times of their own as given, the first first, none twice.
pub(crate) fn at_times(given: &[RemindAt]) -> Vec<RemindAt> {
    let mut at = given.to_vec();
    at.sort();
    at.dedup();
    at
}

pub(crate) fn both_due() -> ApiError {
    ApiError::invalid("due 和 due_after 只能给一个：截止日期，或者前置完成后几天内", Some("due_after".into()))
}

/// The due date `t`'s `due_after` gives it, when it has that: none while its prerequisites are not all
/// done, then counting from the day they were, or its start if later (docs/design/dates.md#前置完成后的截止).
pub(super) fn due_after(t: &Task) -> Option<Option<Due>> {
    let after = t.due_after.as_ref()?;
    Some(t.unlocked_on.as_ref().map(|on| after.due(t.start.as_ref().filter(|s| *s > on).unwrap_or(on))))
}

/// A repeating task is never done: done, it moves on (docs/design/repeat.md#重复); nor due where it can't repeat
/// from, which `Due` takes care of as given, and an undo may come to. It does not start after it is due,
/// a due date without a time lasting all day (docs/design/dates.md#开始时间).
pub(super) fn check_task(t: &Task) -> Result<(), ApiError> {
    if t.done_at.is_some() && t.due.as_ref().is_some_and(|d| d.repeat.is_some()) {
        let ended = if t.dropped { "放弃" } else { "完成" };
        return Err(ApiError::invalid(format!("“{}”已{ended}，不能设置重复，请先重新打开", t.title.as_str()), Some("due".into())));
    }
    if let Some(d) = &t.due
        && let Some(m) = d.repeat.as_ref().and_then(|r| r.misfit(&d.date))
    {
        return Err(ApiError::invalid(format!("“{}”的{m}", t.title.as_str()), Some("due".into())));
    }
    if t.reminder_count() > MAX_REMINDERS {
        return Err(ApiError::invalid(format!("“{}”的提醒最多 {MAX_REMINDERS} 个", t.title.as_str()), Some("remind".into())));
    }
    if t.start_time.is_none() && t.start_reminders.iter().any(|r| matches!(r, Remind::Before(_))) {
        return Err(ApiError::invalid("提前多久的提醒需要具体的开始时间", Some("start_reminders".into())));
    }
    let Some(start) = &t.start else {
        return match (&t.start_time, t.start_reminders.is_empty()) {
            (Some(_), _) => Err(ApiError::invalid("设置开始时刻须先有开始日期", Some("start_time".into()))),
            (None, false) => Err(ApiError::invalid("按开始时间算的提醒需要开始时间", Some("start_reminders".into()))),
            (None, true) => Ok(()),
        };
    };
    // Due the day they are done, it may come to be due the day it starts (docs/design/dates.md#前置完成后的截止).
    if let Some(DueAfter { days: 0, at: Some(at), .. }) = &t.due_after
        && t.start_time.as_ref().is_some_and(|time| *time > at.time)
    {
        return Err(ApiError::invalid(format!("“{}”的开始时间不能晚于截止时间", t.title.as_str()), Some("start_time".into())));
    }
    let Some(due) = &t.due else { return Ok(()) };
    if *start > due.date {
        return Err(ApiError::invalid(format!("“{}”的开始日期不能晚于截止日期", t.title.as_str()), Some("start".into())));
    }
    if *start == due.date
        && let (Some(time), Some(at)) = (&t.start_time, &due.at)
        && *time > at.time
    {
        return Err(ApiError::invalid(format!("“{}”的开始时间不能晚于截止时间", t.title.as_str()), Some("start_time".into())));
    }
    Ok(())
}

/// The people `users` may be assigned to a task in `space`: some of those it is shared with.
pub(super) fn check_assignees(space: &Space, users: &[String]) -> Result<(), ApiError> {
    if users.is_empty() {
        return Ok(());
    }
    if !space.shared() {
        return Err(ApiError::invalid("只有共享的文件夹里的事项能设负责人", Some("assignees".into())));
    }
    if !users.iter().all(|u| space.sees(u)) {
        return Err(ApiError::invalid("负责人须是这个文件夹的共享人或成员", Some("assignees".into())));
    }
    Ok(())
}
