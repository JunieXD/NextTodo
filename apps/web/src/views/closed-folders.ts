// Which folders the desktop sidebar's tree has folded away, kept on this device for each user
// (docs/ui.md#布局): locking, or opening the app again, leaves the tree as it was. Tabs hear each
// other's changes by `storage`.

import { useSyncExternalStore } from "react";
import type { FolderId } from "../api/types/FolderId";

const PREFIX = "nexttodo.closedFolders:";
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
const parse = (saved: string | null) => new Set<FolderId | null>(saved ? JSON.parse(saved) : []);

export const useClosedFolders = (user: string) => parse(useSyncExternalStore(subscribe, () => stored(user)));

/** Folds `folder` away, or unfolds it. Only it: another tab may know of folders this one doesn't yet. */
export function setFolderClosed(user: string, folder: FolderId | null, closed: boolean) {
  const all = parse(stored(user));
  if (closed) all.add(folder);
  else all.delete(folder);
  if (all.size) localStorage.setItem(PREFIX + user, JSON.stringify([...all]));
  else localStorage.removeItem(PREFIX + user);
  for (const l of listeners) l();
}
