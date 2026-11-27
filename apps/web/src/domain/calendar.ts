// What the calendar shows (docs/design.md#日历): what the user is to do each day, the times repeating
// tasks will be due later included, and what they did each day. What is due is also in the core
// library, `crates/core/src/calendar.rs`, for the agenda; both are checked against
// `fixtures/board.json`.

import type { Op } from "../api/types/Op";
import type { Completion } from "../api/types/Completion";
import type { Task } from "../api/types/Task";
import { type Board, cmp } from "./board";
import { day, isoDate } from "./dates";
import { daysBefore, nextDue } from "./repeat";

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

/** The user's tasks due from `from` to `to` (`YYYY-MM-DD`, both in), by day: those with a time first
 * by the time, then in folder order and manual order. A repeating one is due again each time on from
 * its due date, as if done on the day, up to the day it repeats until. */
export function dueBetween(board: Board, from: string, to: string): Map<string, DueOn[]> {
  const days = new Map<string, DueOn[]>();
  for (const task of board.sort(board.snapshot.tasks.filter((t) => t.done_at === null && board.mine(t)))) {
    if (!task.due) continue;
    const { date, repeat } = task.due;
    if (date >= from && date <= to) add(days, date, { task, later: false });
    for (let at = date; repeat; ) {
      const next = nextDue(repeat, at, at);
      if (!next || next > to) break;
      if (next >= from) add(days, next, { task, later: true });
      at = next;
    }
  }
  // Stable: in folder order and manual order at the same time.
  const time = (d: DueOn) => d.task.due!.time ?? "24:00";
  for (const list of days.values()) list.sort((a, b) => cmp(time(a), time(b)));
  return days;
}

/** The user's tasks given up, by the day they were due, in folder order and manual order: listed
 * with what is due then (docs/design.md#放弃). */
export function droppedByDay(board: Board): Map<string, Task[]> {
  const days = new Map<string, Task[]>();
  for (const task of board.sort(board.snapshot.tasks.filter((t) => t.dropped && board.mine(t)))) if (task.due) add(days, task.due.date, task);
  return days;
}

/** The user's open tasks due before `today`, in the order of `Board.byDue`. */
export const overdue = (board: Board, today: string) =>
  board.byDue(board.snapshot.tasks.filter((t) => t.done_at === null && t.due !== null && t.due.date < today && board.mine(t)));

/** What is due over `days` days from the board's today, by day in order, and before it, what is
 * `overdue`. */
export function agenda(board: Board, days: number) {
  const { today } = board.now;
  const d = day(today);
  const due = dueBetween(board, today, isoDate(new Date(d.getFullYear(), d.getMonth(), d.getDate() + days - 1)));
  return { overdue: overdue(board, today), days: [...due].sort(([a], [b]) => cmp(a, b)) };
}

/** The record of the time that ended `task`, done for good the last time it repeated: its due date
 * stays that time's, and that time done more than once, the record is the latest. None for a task
 * given up, which was not done; the times it was done before were. */
export const endingRecord = (board: Board, task: Task): Completion | undefined =>
  task.done_at === null || task.dropped
    ? undefined
    : board.completions(task.id).reduce<Completion | undefined>((l, r) => (r.due === task.due?.date && (!l || r.again > l.again) ? r : l), undefined);

export function completionTimeOps(board: Board, task: Task, done_at: number): Op[] {
  const record = endingRecord(board, task);
  return [
    { op: "task.update", id: task.id, done_at },
    ...(record ? [editCompletion(record, isoDate(new Date(done_at)))] : []),
  ];
}

export function completionDateOps(board: Board, task: Task, record: Completion, done: string): Op[] {
  const ops = [editCompletion(record, done)];
  const ended = endingRecord(board, task);
  if (ended?.due !== record.due || ended.again !== record.again || task.done_at === null) return ops;
  const at = new Date(task.done_at);
  at.setDate(at.getDate() + daysBefore(isoDate(at), done));
  ops.push({ op: "task.update", id: task.id, done_at: at.getTime() });
  return ops;
}

function editCompletion({ task_id, due, again }: Completion, done: string): Op {
  return { op: "completion.update", task_id, due, again, done };
}

/** What the user did, by day: the tasks done, on the day they were here, and the times repeating tasks
 * were done, those given up since too. A task done for good the last time it repeated is there once,
 * as the task, on the day of its `endingRecord`: `done_at` is when the server took it, which may be a
 * day later for one done offline. */
export function doneByDay(board: Board): Map<string, DoneOn[]> {
  const days = new Map<string, DoneOn[]>();
  const last = new Set<Completion>();
  for (const task of board.snapshot.tasks) {
    if (task.done_at === null || task.dropped || !board.mine(task)) continue;
    const record = endingRecord(board, task);
    if (record) last.add(record);
    add(days, record?.done ?? isoDate(new Date(task.done_at)), { task });
  }
  for (const record of board.snapshot.completions) {
    const task = board.task(record.task_id);
    if (task && board.mine(task) && !last.has(record)) add(days, record.done, { task, record });
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
