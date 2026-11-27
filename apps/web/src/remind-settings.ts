// What this device does about reminders (docs/design/dates.md#提醒): those a task gets as it is given a due
// date, whether reminders hide what they are of, and the daily digest. Each device keeps its own, as it
// does the theme.

import { useSyncExternalStore } from "react";
import type { DefaultReminders } from "./api/types/DefaultReminders";
import type { Due } from "./api/types/Due";
import { inOrder } from "./domain/dates";

const DEFAULTS = "nexttodo.defaultReminders";
const HIDE = "nexttodo.hideReminders";

const listeners = new Set<() => void>();

/** Calls `f` each time these settings change; returns how to stop. */
export function onRemindSettings(f: () => void): () => void {
  listeners.add(f);
  // Changed in another tab, or the desktop app's other window.
  const other = (e: StorageEvent) => (e.key === DEFAULTS || e.key === HIDE || e.key === DIGEST) && f();
  addEventListener("storage", other);
  return () => {
    listeners.delete(f);
    removeEventListener("storage", other);
  };
}

const changed = () => {
  for (const l of listeners) l();
};

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

export const defaultReminders = kept<DefaultReminders>(DEFAULTS, '{"timed":[],"dated":[]}');

/** A due date on the calendar with this device's defaults for a timed or untimed task. */
export function dueWithDefaults(date: string, time?: string): Due {
  const defaults = defaultReminders();
  const reminders = time === undefined ? inOrder(defaults.dated, undefined) : inOrder(defaults.timed, time);
  return { date, ...(time !== undefined && { time }), ...(reminders.length > 0 && { reminders }) };
}

/** Those for AI organizing to give the tasks it makes with a due date and no reminders
 * (docs/ai.md#提示词和结果): none when there are none. */
export function jobReminders(): DefaultReminders | undefined {
  const d = defaultReminders();
  return d.timed.length || d.dated.length ? d : undefined;
}

export function setDefaultReminders(d: DefaultReminders) {
  localStorage.setItem(DEFAULTS, JSON.stringify(d));
  changed();
}

export const hideReminders = () => localStorage.getItem(HIDE) === "1";

export function setHideReminders(on: boolean) {
  if (on) localStorage.setItem(HIDE, "1");
  else localStorage.removeItem(HIDE);
  changed();
}

/** The daily digest: whether on, and at what time of the day. */
export interface Digest {
  on: boolean;
  time: string;
}

const DIGEST = "nexttodo.digest";

export const dailyDigest = kept<Digest>(DIGEST, '{"on":false,"time":"21:00"}');

export function setDailyDigest(d: Digest) {
  localStorage.setItem(DIGEST, JSON.stringify(d));
  changed();
}

export const useDefaultReminders = () => useSyncExternalStore(onRemindSettings, defaultReminders);
export const useHideReminders = () => useSyncExternalStore(onRemindSettings, hideReminders);
export const useDailyDigest = () => useSyncExternalStore(onRemindSettings, dailyDigest);
