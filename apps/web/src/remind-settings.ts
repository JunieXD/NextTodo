// What this device does about reminders (docs/design.md#提醒): those a task gets as it is given a due
// date, and whether reminders hide what they are of. Each device keeps its own, as it does the theme.

import { useSyncExternalStore } from "react";
import type { RemindBefore } from "./api/types/RemindBefore";
import type { RemindOn } from "./api/types/RemindOn";

const DEFAULTS = "nexttodo.defaultReminders";
const HIDE = "nexttodo.hideReminders";

/** For a task due at a time, and for one due on a date alone. */
export interface DefaultReminders {
  timed: RemindBefore[];
  dated: RemindOn[];
}

const listeners = new Set<() => void>();

/** Calls `f` each time these settings change; returns how to stop. */
export function onRemindSettings(f: () => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

const changed = () => {
  for (const l of listeners) l();
};

// The same object until it changes, as React wants of what it subscribes to.
let defaults: DefaultReminders | undefined;

export const defaultReminders = (): DefaultReminders => (defaults ??= JSON.parse(localStorage.getItem(DEFAULTS) ?? '{"timed":[],"dated":[]}'));

export function setDefaultReminders(d: DefaultReminders) {
  localStorage.setItem(DEFAULTS, JSON.stringify(d));
  defaults = d;
  changed();
}

export const hideReminders = () => localStorage.getItem(HIDE) === "1";

export function setHideReminders(on: boolean) {
  if (on) localStorage.setItem(HIDE, "1");
  else localStorage.removeItem(HIDE);
  changed();
}

export const useDefaultReminders = () => useSyncExternalStore(onRemindSettings, defaultReminders);
export const useHideReminders = () => useSyncExternalStore(onRemindSettings, hideReminders);
