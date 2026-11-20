//! What the user is to do each day (docs/design.md#日历): the agenda the `agenda` tool and the Android
//! widget give. The web app has a TypeScript twin, `apps/web/src/domain/calendar.ts`; both are checked
//! against `fixtures/board.json`.

use std::collections::BTreeMap;

use crate::board::Board;
use crate::model::{Date, Task};

/// A task due on a day: when it is due now, or, `later`, a time a repeating one will be due after that.
pub struct DueOn<'a> {
    pub task: &'a Task,
    pub later: bool,
}

/// The user's tasks due from `from` to `to`, both in, by day: those with a time first by the time,
/// then in folder order and manual order. A repeating one is due again each time on from its due
/// date, as if done on the day, up to the day it repeats until.
pub fn due_between<'a>(b: &Board<'a>, from: &Date, to: &Date) -> BTreeMap<Date, Vec<DueOn<'a>>> {
    let mut tasks: Vec<&Task> = b.tasks().filter(|t| t.done_at.is_none() && b.mine(t)).collect();
    b.sort(&mut tasks);
    let mut days: BTreeMap<Date, Vec<DueOn>> = BTreeMap::new();
    for task in tasks {
        let Some(due) = &task.due else { continue };
        if (from..=to).contains(&&due.date) {
            days.entry(due.date.clone()).or_default().push(DueOn { task, later: false });
        }
        let mut at = due.date.clone();
        while let Some(next) = due.repeat.as_ref().and_then(|r| r.next(&at, &at)).filter(|next| next <= to) {
            if next >= *from {
                days.entry(next.clone()).or_default().push(DueOn { task, later: true });
            }
            at = next;
        }
    }
    for list in days.values_mut() {
        // Stable: in folder order and manual order at the same time.
        list.sort_by_key(|d| d.task.due.as_ref().and_then(|due| due.at.as_ref()).map_or("24:00", |at| at.time.as_str()));
    }
    days
}

/// What is due over the next days, from today on, and what is overdue.
pub struct Agenda<'a> {
    /// The user's open tasks due before today, in the order of `Board::by_due`.
    pub overdue: Vec<&'a Task>,
    /// The days with something due.
    pub days: BTreeMap<Date, Vec<DueOn<'a>>>,
}

/// The agenda for `days` days, 1 or more, from the board's today.
pub fn agenda<'a>(b: &Board<'a>, days: u32) -> Agenda<'a> {
    let today = &b.now().today;
    let mut overdue: Vec<&Task> =
        b.tasks().filter(|t| t.done_at.is_none() && b.mine(t) && t.due.as_ref().is_some_and(|d| d.date < *today)).collect();
    b.by_due(&mut overdue);
    Agenda { overdue, days: due_between(b, today, &today.plus(days - 1)) }
}
