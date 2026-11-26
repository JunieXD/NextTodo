// When to remind of tasks (docs/design.md#提醒): each reminder of a task, at the time it says, by its
// due date, its start, or at a time of its own. The phone apps hand these to the device; the desktop app
// shows them while it runs.

import type { Due } from "../api/types/Due";
import type { Occurrence } from "../api/types/Occurrence";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";
import { day, formatDue, isoDate, remindAt, remindAtLabel, remindAtTime, remindLabel, startRemindLabel } from "./dates";
import { occurrence } from "./repeat";

export interface Reminder {
  at: Date;
  /** The time reminded of, should the task repeat: done from one reminder, the others of that time
   * go too, and a time done already is not done again (docs/api.md#改动). */
  occurrence?: Occurrence;
  /** Whether it can be done from the notification: not when a repeating task is also reminded then at
   * a time of its own, which is of none of its times. Such a reminder has no `occurrence` either, so
   * that doing or skipping a time doesn't take it away (docs/design.md#提醒). */
  completable: boolean;
  title: string;
  body: string;
  task: Task;
}

/** What a reminder says when its content is hidden (docs/design.md#提醒). */
export const HIDDEN = "有事项提醒";

/** The reminders of an unfinished task of the user's (docs/design.md#共享), one at each moment;
 * `hide`, saying only that there is something. */
export function remindersOf(board: Board, t: Task, hide: boolean): Reminder[] {
  if (t.done_at !== null || !board.mine(t)) return [];
  const due = t.due;
  const start = startOf(t);
  const until = (d: Due, what: string) => (at: Date) => `${formatDue(d, at)}${d.time ? " " : ""}${what}`;
  // What a moment says is what the first of its reminders, by the due date, the start or at a time of
  // its own, goes by; `own`, whether one is at a time of its own.
  const moments = new Map<number, { says: (at: Date) => string; own: boolean }>();
  const add = (at: Date, says: (at: Date) => string, own: boolean) => {
    const m = moments.get(at.getTime());
    if (m) m.own ||= own;
    else moments.set(at.getTime(), { says, own });
  };
  if (due) for (const r of due.reminders ?? []) add(remindAt(due, r), until(due, "截止"), false);
  if (start) for (const r of t.start_reminders ?? []) add(remindAt(start, r), until(start, "开始"), false);
  for (const r of t.remind_at ?? []) add(remindAtTime(r), due ? until(due, "截止") : () => "", true);
  const where = t.folder_id ? board.place(t.folder_id) : "";
  const times = due?.repeat && t.completions ? `已完成 ${t.completions.count} 次` : "";
  return [...moments].map(([ms, { says: when, own }]) => {
    const at = new Date(ms);
    const says = hide ? { title: HIDDEN, body: "" } : { title: t.title, body: [when(at), where, times].filter(Boolean).join(" · ") };
    const completable = !(own && due?.repeat);
    return { at, occurrence: completable ? occurrence(t) : undefined, completable, task: t, ...says };
  });
}

/** When `t` starts, as a due date is: the date, and the time if it has one. */
export const startOf = (t: Task): Due | null => (t.start ? { date: t.start, ...(t.start_time && { time: t.start_time }) } : null);

/** What each of `t`'s reminders says, `now`, in the order they come; those by a due date still to come
 * after its prerequisites last (docs/ui.md#详情). */
export function reminderLabels(t: Task, now: Date): string[] {
  const { due } = t;
  const start = startOf(t);
  const dated: [number, string][] = [
    ...(due ? (due.reminders ?? []).map((r): [number, string] => [remindAt(due, r).getTime(), remindLabel(r)]) : []),
    ...(start ? (t.start_reminders ?? []).map((r): [number, string] => [remindAt(start, r).getTime(), startRemindLabel(r)]) : []),
    ...(t.remind_at ?? []).map((r): [number, string] => [remindAtTime(r).getTime(), remindAtLabel(r, now)]),
  ];
  const undated = due ? [] : (t.due_after?.reminders ?? []).map(remindLabel);
  return [...dated.sort(([a], [b]) => a - b).map(([, label]) => label), ...undated];
}

/** Every reminder there is, those past too, earliest first. */
export function reminders(board: Board, hide: boolean): Reminder[] {
  return board.snapshot.tasks.flatMap((t) => remindersOf(board, t, hide)).sort((a, b) => a.at.getTime() - b.at.getTime());
}

/** A daily digest (docs/design.md#提醒): of a day, the tasks left to do by its end, each with its
 * time, as reminders have it, for the device to take out one done there. */
export interface DailyDigest {
  at: Date;
  tasks: { task: TaskId; occurrence?: Occurrence; title: string }[];
}

/** How many days from today the digests are reckoned for: the app may not run in between. */
export const DIGEST_DAYS = 7;

/** The digests at `time` of each day from yesterday, whose an alarm held back may not have shown yet,
 * to `DIGEST_DAYS` from today: of the user's open tasks due by that day, earliest first; none on a
 * day with none. */
export function digests(board: Board, time: string): DailyDigest[] {
  const open = board
    .byDue(board.snapshot.tasks.filter((t) => t.done_at === null && board.mine(t)))
    .flatMap((t) => (t.due ? [{ task: t.id, occurrence: occurrence(t), title: t.title, date: t.due.date }] : []));
  const today = day(board.now.today);
  const [h, m] = time.split(":").map(Number);
  return Array.from({ length: DIGEST_DAYS + 1 }, (_, i) => new Date(today.getFullYear(), today.getMonth(), today.getDate() + i - 1, h, m)).flatMap((at) => {
    const left = open.filter((t) => t.date <= isoDate(at));
    return left.length ? [{ at, tasks: left.map(({ task, occurrence, title }) => ({ task, occurrence, title })) }] : [];
  });
}

/** What a digest of the tasks titled `titles` says: how many, and unless `hide`, which. The phone apps say it
 * alike of what is left once some are done there. */
export const digestText = (titles: string[], hide: boolean) => ({ title: `今天还有 ${titles.length} 项未完成`, body: hide ? "" : titles.join("、") });
