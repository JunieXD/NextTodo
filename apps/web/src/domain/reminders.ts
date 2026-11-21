// When to remind of deadlines (docs/design.md#提醒): each reminder of a task, at the time it says. The
// phone apps hand these to the device; the desktop app shows them while it runs.

import type { Task } from "../api/types/Task";
import type { Board } from "./board";
import { deadline, formatDue, remindAt } from "./dates";

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
