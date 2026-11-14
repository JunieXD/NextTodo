// When to remind of deadlines: a task due at a time, with a reminder set, is reminded of that long
// before. The Android app hands these to the device; the desktop app shows them while it runs.

import type { Task } from "../api/types/Task";
import type { Board } from "./board";
import { deadline, formatDue } from "./dates";

export interface Reminder {
  at: Date;
  title: string;
  body: string;
  task: Task;
}

/** The reminder of an unfinished task of the user's with one (docs/design.md#共享). */
export function reminder(board: Board, t: Task): Reminder | null {
  const remind = t.due?.remind_before;
  if (t.done_at !== null || !board.mine(t) || !t.due || remind === undefined) return null;
  const at = new Date(deadline(t.due).getTime() - remind * 60_000);
  const where = t.folder_id ? ` · ${board.place(t.folder_id)}` : "";
  const times = t.due.repeat && t.completions ? ` · 已完成 ${t.completions.count} 次` : "";
  return { at, title: t.title, body: `${formatDue(t.due, at)} 截止${where}${times}`, task: t };
}

/** Every reminder there is, those past too, earliest first. */
export function reminders(board: Board): Reminder[] {
  return board.snapshot.tasks.flatMap((t) => reminder(board, t) ?? []).sort((a, b) => a.at.getTime() - b.at.getTime());
}
