// What the calendar's time grid shows (docs/ui/calendar.md): a week of the user's tasks by the hour they
// are due, in half-hour cells, those due on a date alone in a row of their own.

import type { Completion } from "../api/types/Completion";
import type { Task } from "../api/types/Task";
import { type Board, cmp } from "./board";
import { dueBetween, endingRecord } from "./calendar";
import { dueKind } from "./dates";

/** How a block looks, by what it is: also how much it matters, the first most. */
export const LOOKS = ["overdue", "ready", "waiting", "later", "done", "dropped"] as const;
export type Look = (typeof LOOKS)[number];

/** A task due on a day: as it is now, a time a repeating one will be due (`later`), or one it was done
 * (`record`). */
export interface Block {
  task: Task;
  look: Look;
  /** `HH:MM`, or none for one due on the day alone. */
  time: string | null;
  later?: boolean;
  record?: Completion;
}

/** The half hour of the day a time is in, from 0 at midnight to 47. */
export const slotOf = (time: string) => {
  const [h, m] = time.split(":").map(Number);
  return h * 2 + (m >= 30 ? 1 : 0);
};

/** `HH:MM` where half hour `slot` begins. */
export const slotTime = (slot: number) => `${String(Math.floor(slot / 2)).padStart(2, "0")}:${slot % 2 ? "30" : "00"}`;

/** The blocks of each of `days` (`YYYY-MM-DD`, in order), by the time they are due and then as the
 * lists have them: the user's tasks due that day, open, done or given up; the times repeating ones will
 * be due; and those they were done, on the day that time was due, at the task's time. A task done
 * for good the last time it repeated is there once, as the task, as in `doneByDay`. */
export function weekBlocks(board: Board, days: string[], now: Date): Map<string, Block[]> {
  const week = new Map(days.map((d) => [d, [] as Block[]]));
  const [from, to] = [days[0], days[days.length - 1]];
  for (const [day, list] of dueBetween(board, from, to)) {
    for (const { task, later } of list) {
      const status = board.status(task);
      const look: Look = later ? "later" : dueKind(task.due!, now) === "overdue" ? "overdue" : status === "ready" ? "ready" : "waiting";
      week.get(day)!.push({ task, look, time: task.due!.time ?? null, ...(later && { later }) });
    }
  }
  const ended = board.sort(board.snapshot.tasks.filter((t) => t.done_at !== null && t.due && t.due.date >= from && t.due.date <= to && board.mine(t)));
  const ending = new Set<Completion>();
  for (const task of ended) {
    week.get(task.due!.date)!.push({ task, look: task.dropped ? "dropped" : "done", time: task.due!.time ?? null });
    const record = endingRecord(board, task);
    if (record) ending.add(record);
  }
  for (const record of board.snapshot.completions) {
    const task = board.task(record.task_id);
    const list = week.get(record.due);
    if (task && list && board.mine(task) && !ending.has(record)) list.push({ task, look: "done", time: task.due?.time ?? null, record });
  }
  for (const list of week.values()) list.sort((a, b) => cmp(a.time ?? "", b.time ?? ""));
  return week;
}

/** `blocks` in one place, as many as `room` takes: when there are more, the last of those shown and the
 * rest go together in `merged`, which looks as the one of them that matters most. With room for one,
 * all of them go together. */
export function fit(blocks: Block[], room: number): { shown: Block[]; merged: Block[] } {
  if (blocks.length <= room) return { shown: blocks, merged: [] };
  const n = room <= 1 ? 0 : room - 1;
  return { shown: blocks.slice(0, n), merged: blocks.slice(n) };
}

/** What matters most of `blocks`. */
export const firstLook = (blocks: Block[]): Look => LOOKS[Math.min(...blocks.map((b) => LOOKS.indexOf(b.look)))];

/** Where the grid opens, in minutes from midnight: this week, an hour and a half before now; another,
 * an hour before the first thing due at a time that week; with none, 8:00. */
export function opensAt(week: Map<string, Block[]>, now: Date, today: string): number {
  if (week.has(today)) return Math.max(0, now.getHours() * 60 + now.getMinutes() - 90);
  const times = [...week.values()].flat().flatMap((b) => (b.time ? [b.time] : []));
  if (!times.length) return 8 * 60;
  const [h, m] = times.reduce((a, b) => (b < a ? b : a)).split(":").map(Number);
  return Math.max(0, h * 60 + m - 60);
}
