// What is done about reminders (docs/design/dates.md#提醒): those a task gets as it is given a due date, which
// go with the account (`prefs.ts`); and, which each device keeps its own of, as it does the theme,
// whether reminders hide what they are of, and the daily digest.

import { useSyncExternalStore } from "react";
import type { DefaultReminders } from "./api/types/DefaultReminders";
import type { Due } from "./api/types/Due";
import { inOrder } from "./domain/dates";
import { keep, onKept } from "./kept";
import type { Board } from "./domain/board";
import { prefsOf } from "./prefs";
const HIDE = "nexttodo.hideReminders";

/** Calls `f` each time these settings change; returns how to stop. */
export const onRemindSettings = (f: () => void) =>
  onKept((key) => (key === null || key === HIDE || key === DIGEST) && f());

/** Reads what is kept under `key`, or else `none`: the same object until what is kept changes, here
 * or in another window, as React wants of what it subscribes to. */
function kept<T>(key: string, none: string): () => T {
  let last: { saved: string | null; value: T } | undefined;
  return () => {
    const saved = localStorage.getItem(key);
    if (last?.saved !== saved) last = { saved, value: JSON.parse(saved ?? none) };
    return last.value;
  };
}

export const defaultReminders = (board: Board): DefaultReminders => prefsOf(board).reminders ?? { timed: [], dated: [] };

/** A due date on the calendar with the defaults for a timed or untimed task. */
export function dueWithDefaults(board: Board, date: string, time?: string): Due {
  const defaults = defaultReminders(board);
  const reminders = time === undefined ? inOrder(defaults.dated, undefined) : inOrder(defaults.timed, time);
  return { date, ...(time !== undefined && { time }), ...(reminders.length > 0 && { reminders }) };
}

/** Those for AI organizing to give the tasks it makes with a due date and no reminders
 * (docs/ai.md#提示词和结果): none when there are none. */
export function jobReminders(board: Board): DefaultReminders | undefined {
  const d = defaultReminders(board);
  return d.timed.length || d.dated.length ? d : undefined;
}

export const hideReminders = () => localStorage.getItem(HIDE) === "1";

export const setHideReminders = (on: boolean) => keep(HIDE, on ? "1" : null);

/** The daily digest: whether on, and at what time of the day. */
export interface Digest {
  on: boolean;
  time: string;
}

const DIGEST = "nexttodo.digest";

export const dailyDigest = kept<Digest>(DIGEST, '{"on":false,"time":"21:00"}');

export const setDailyDigest = (d: Digest) => keep(DIGEST, JSON.stringify(d));

export const useHideReminders = () => useSyncExternalStore(onRemindSettings, hideReminders);
export const useDailyDigest = () => useSyncExternalStore(onRemindSettings, dailyDigest);
