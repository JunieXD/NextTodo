// Which of “接下来”'s groups are folded away, kept on this device for each user
// (docs/design.md#接下来). The menu bar panel and other tabs hear changes by `storage`.

import { useSyncExternalStore } from "react";
import type { DueKind } from "../domain/dates";

/** A group of “接下来”: by when its tasks are due, or with none. */
export type DueGroup = DueKind | "none";

const PREFIX = "nexttodo.closedGroups:";
const listeners = new Set<() => void>();

function subscribe(l: () => void) {
  listeners.add(l);
  addEventListener("storage", l);
  return () => {
    listeners.delete(l);
    removeEventListener("storage", l);
  };
}

const stored = (user: string) => localStorage.getItem(PREFIX + user);
const parse = (saved: string | null) => new Set<DueGroup>(saved ? JSON.parse(saved) : []);

export const useClosedGroups = (user: string) => parse(useSyncExternalStore(subscribe, () => stored(user)));

export function setGroupClosed(user: string, group: DueGroup, closed: boolean) {
  const all = parse(stored(user));
  if (closed) all.add(group);
  else all.delete(group);
  if (all.size) localStorage.setItem(PREFIX + user, JSON.stringify([...all]));
  else localStorage.removeItem(PREFIX + user);
  for (const l of listeners) l();
}
