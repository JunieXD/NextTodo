// When to remind of deadlines: a task due at a time, with a reminder set, is reminded of that long
// before. The Android app schedules these on the device; the macOS app shows them while it runs.

import type { Task } from "../api/types/Task";
import type { Board } from "./board";
import { deadline, formatDue } from "./dates";

export interface Reminder {
  /** The task's number, which the server gives out: unique per user and an integer. */
  id: number;
  at: Date;
  title: string;
  body: string;
  task: string;
}

/** The reminder of an unfinished task with one. Tasks not synced yet have no number and wait. */
export function reminder(board: Board, t: Task): Reminder | null {
  const remind = t.due?.remind_before;
  if (t.done_at !== null || t.number === 0 || !t.due || remind === undefined) return null;
  const at = new Date(deadline(t.due).getTime() - remind * 60_000);
  const where = t.folder_id ? ` · ${board.place(t.folder_id)}` : "";
  return { id: t.number, at, title: t.title, body: `${formatDue(t.due, at)} 截止${where}`, task: t.id };
}

/** The reminders due after `after`, earliest first. */
export function reminders(board: Board, after: Date): Reminder[] {
  return board.snapshot.tasks
    .flatMap((t) => reminder(board, t) ?? [])
    .filter((r) => r.at > after)
    .sort((a, b) => a.at.getTime() - b.at.getTime());
}
