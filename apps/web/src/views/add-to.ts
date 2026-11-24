// Where the quick add bars that have a folder to pick put a task (docs/ui.md#其他): the folder last
// picked, kept on this device for each user and shared by all of them; or, fixed in the settings, that
// one, which picking another overrides until the next add. Windows hear each other's changes by
// `storage`.

import { useSyncExternalStore } from "react";
import type { FolderId } from "../api/types/FolderId";
import type { Board } from "../domain/board";

const PICKED = "nexttodo.addTo:";
const FIXED = "nexttodo.addToFixed:";
const listeners = new Set<() => void>();

function subscribe(l: () => void) {
  listeners.add(l);
  addEventListener("storage", l);
  return () => {
    listeners.delete(l);
    removeEventListener("storage", l);
  };
}

const changed = () => {
  for (const l of listeners) l();
};

const parse = (saved: string | null): FolderId | null | undefined => (saved === null ? undefined : JSON.parse(saved));

const useKept = (key: string) => parse(useSyncExternalStore(subscribe, () => localStorage.getItem(key)));

/** The folder fixed in the settings, still there or not; undefined when the bars go on from the one
 * last picked. */
export const useFixedAddTo = (user: string) => useKept(FIXED + user);

/** The folder picked, or else the one fixed, if it is still there; undefined for the bar's own default. */
export function useAddTo(board: Board): FolderId | null | undefined {
  const picked = useKept(PICKED + board.me);
  const fixed = useFixedAddTo(board.me);
  return [picked, fixed].find((f) => f !== undefined && board.hasFolder(f));
}

export function pickAddTo(user: string, f: FolderId | null) {
  localStorage.setItem(PICKED + user, JSON.stringify(f));
  changed();
}

/** After a bar has added to `used`: a folder picked over the fixed one was for that once. Not one
 * another window has picked since. */
export function addedTo(user: string, used: FolderId | null) {
  if (localStorage.getItem(FIXED + user) === null || localStorage.getItem(PICKED + user) !== JSON.stringify(used)) return;
  localStorage.removeItem(PICKED + user);
  changed();
}

export function fixAddTo(user: string, f: FolderId | null) {
  localStorage.setItem(FIXED + user, JSON.stringify(f));
  localStorage.removeItem(PICKED + user);
  changed();
}

/** Has the bars go on from the folder last picked, as if it were `shown`, the one they show now, so
 * that they stay as they are; undefined when that is their own default. */
export function unfixAddTo(user: string, shown: FolderId | null | undefined) {
  localStorage.removeItem(FIXED + user);
  if (shown !== undefined) localStorage.setItem(PICKED + user, JSON.stringify(shown));
  changed();
}
