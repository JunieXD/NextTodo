// When to remind of deadlines (docs/design.md#提醒): each reminder of a task, at the time it says. The
// phone apps hand these to the device; the desktop app shows them while it runs.

import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";
import { day, deadline, formatDue, isoDate, remindAt } from "./dates";

export interface Reminder {
  at: Date;
  /** When the time reminded of is due: done from one reminder, the others of that time go too. */
  due: Date;
  title: string;
  body: string;
  task: Task;
}

/** What a reminder says when its content is hidden (docs/design.md#提醒). */
export const HIDDEN = "有事项到期";

/** The reminders of an unfinished task of the user's (docs/design.md#共享), one at each moment;
 * `hide`, saying only that something is due. */
export function remindersOf(board: Board, t: Task, hide: boolean): Reminder[] {
  const due = t.done_at === null && board.mine(t) ? t.due : null;
  if (!due?.reminders) return [];
  const where = t.folder_id ? ` · ${board.place(t.folder_id)}` : "";
  const times = due.repeat && t.completions ? ` · 已完成 ${t.completions.count} 次` : "";
  return [...new Set(due.reminders.map((r) => remindAt(due, r).getTime()))].map((ms) => {
    const at = new Date(ms);
    const says = hide ? { title: HIDDEN, body: "" } : { title: t.title, body: `${formatDue(due, at)}${due.time ? " " : ""}截止${where}${times}` };
    return { at, due: deadline(due), task: t, ...says };
  });
}

/** Every reminder there is, those past too, earliest first. */
export function reminders(board: Board, hide: boolean): Reminder[] {
  return board.snapshot.tasks.flatMap((t) => remindersOf(board, t, hide)).sort((a, b) => a.at.getTime() - b.at.getTime());
}

/** A daily digest (docs/design.md#提醒): of a day, the tasks left to do by its end, each with when its
 * time is due, as reminders have it, for the device to take out one done there. */
export interface DailyDigest {
  at: Date;
  tasks: { task: TaskId; due: Date; title: string }[];
}

/** How many days from today the digests are reckoned for: the app may not run in between. */
export const DIGEST_DAYS = 7;

/** The digests at `time` of each day from yesterday, whose an alarm held back may not have shown yet,
 * to `DIGEST_DAYS` from today: of the user's open tasks due by that day, earliest first; none on a
 * day with none. */
export function digests(board: Board, time: string): DailyDigest[] {
  const open = board
    .byDue(board.snapshot.tasks.filter((t) => t.done_at === null && board.mine(t)))
    .flatMap((t) => (t.due ? [{ task: t.id, due: deadline(t.due), title: t.title, date: t.due.date }] : []));
  const today = day(board.now.today);
  const [h, m] = time.split(":").map(Number);
  return Array.from({ length: DIGEST_DAYS + 1 }, (_, i) => new Date(today.getFullYear(), today.getMonth(), today.getDate() + i - 1, h, m)).flatMap((at) => {
    const left = open.filter((t) => t.date <= isoDate(at));
    return left.length ? [{ at, tasks: left.map(({ task, due, title }) => ({ task, due, title })) }] : [];
  });
}

/** What a digest of the tasks titled `titles` says: how many, and unless `hide`, which. The phone apps say it
 * alike of what is left once some are done there. */
export const digestText = (titles: string[], hide: boolean) => ({ title: `今天还有 ${titles.length} 项没做完`, body: hide ? "" : titles.join("、") });
