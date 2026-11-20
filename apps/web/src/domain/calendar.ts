// What the calendar shows (docs/design.md#日历): what the user is to do each day, the times repeating
// tasks will be due later included, and what they did each day.

import type { Completion } from "../api/types/Completion";
import type { Task } from "../api/types/Task";
import type { Board } from "./board";
import { isoDate } from "./dates";
import { nextDue } from "./repeat";

/** A task due on a day: when it is due now, or, `later`, a time a repeating one will be due after that. */
export interface DueOn {
  task: Task;
  later: boolean;
}

/** Something done on a day: a task, or a time a repeating one was done, as its `record` says. */
export interface DoneOn {
  task: Task;
  record?: Completion;
}

function add<T>(days: Map<string, T[]>, day: string, item: T) {
  const list = days.get(day);
  if (list) list.push(item);
  else days.set(day, [item]);
}

/** The user's tasks due from `from` to `to` (`YYYY-MM-DD`, both in), by day, those with a time first
 * by the time. A repeating one is due again each time on from its due date, as if done on the day. */
export function dueBetween(board: Board, from: string, to: string): Map<string, DueOn[]> {
  const days = new Map<string, DueOn[]>();
  for (const task of board.snapshot.tasks) {
    if (task.done_at !== null || !task.due || !board.mine(task)) continue;
    const { date, repeat } = task.due;
    if (date >= from && date <= to) add(days, date, { task, later: false });
    for (let at = date; repeat; ) {
      const next = nextDue(repeat, at, at);
      if (!next || next > to) break;
      if (next >= from) add(days, next, { task, later: true });
      at = next;
    }
  }
  const time = (d: DueOn) => d.task.due!.time ?? "24:00";
  for (const list of days.values()) list.sort((a, b) => time(a).localeCompare(time(b)) || a.task.title.localeCompare(b.task.title));
  return days;
}

/** What the user did, by day: the tasks done, on the day they were here, and the times repeating tasks
 * were done. A task done for good the last time it repeated is there once, as the task, on the day
 * of that time's record: its due date stays that time's, and `done_at` is when the server took it,
 * which may be a day later for one done offline. */
export function doneByDay(board: Board): Map<string, DoneOn[]> {
  const days = new Map<string, DoneOn[]>();
  const last = (task: Task, record: Completion) => task.done_at !== null && record.due === task.due?.date;
  for (const task of board.snapshot.tasks) {
    if (task.done_at === null || !board.mine(task)) continue;
    const record = board.completions(task.id).find((r) => last(task, r));
    add(days, record?.done ?? isoDate(new Date(task.done_at)), { task });
  }
  for (const record of board.snapshot.completions) {
    const task = board.task(record.task_id);
    if (task && board.mine(task) && !last(task, record)) add(days, record.done, { task, record });
  }
  return days;
}

/** How deep a day is tinted for `n` things done: 0 for none, then 1, 2–3, 4–6, 7 and more. */
export const heat = (n: number) => (n === 0 ? 0 : n === 1 ? 1 : n <= 3 ? 2 : n <= 6 ? 3 : 4);

/** `按时`, `晚 3 天`, `提前 2 天`: when a repeating task was done, against when it was due. */
export function lateness(c: Completion): string {
  const day = (s: string) => Date.parse(`${s}T00:00:00Z`) / 86_400_000;
  const late = day(c.done) - day(c.due);
  return late === 0 ? "按时" : late > 0 ? `晚 ${late} 天` : `提前 ${-late} 天`;
}
