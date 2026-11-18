// Which folders “接下来” lists, picked on this device for each user (docs/ui.md#接下来的文件夹筛选):
// the root, which is all of them, until others are. The menu bar panel shares the pick with the main
// window, hearing it change there by `storage`.

import { useSyncExternalStore } from "react";
import type { FolderId } from "./api/types/FolderId";
import type { Task } from "./api/types/Task";
import type { Board } from "./domain/board";

const PREFIX = "next-folders:";
const listeners = new Set<() => void>();

/** Calls `l` whenever the pick changes, in this window or another. */
export function onNextFolders(l: () => void) {
  listeners.add(l);
  addEventListener("storage", l);
  return () => {
    listeners.delete(l);
    removeEventListener("storage", l);
  };
}

const stored = (user: string) => localStorage.getItem(PREFIX + user);

/** Those picked that are still there, deleted ones and ones no longer shared with the user left out;
 * all of them when none is. */
function live(board: Board, saved: string | null): (FolderId | null)[] {
  const picked = (JSON.parse(saved ?? "[null]") as (FolderId | null)[]).filter((f) => board.hasFolder(f));
  return picked.length ? picked : [null];
}

export const nextFolders = (board: Board) => live(board, stored(board.me));

export const useNextFolders = (board: Board) => live(board, useSyncExternalStore(onNextFolders, () => stored(board.me)));

export function setNextFolders(user: string, folders: (FolderId | null)[]) {
  if (folders.includes(null)) localStorage.removeItem(PREFIX + user);
  else localStorage.setItem(PREFIX + user, JSON.stringify(folders));
  for (const l of listeners) l();
}

/** The ready tasks in `folders` and the folders inside them. */
export const readyIn = (board: Board, folders: (FolderId | null)[]): Task[] =>
  board.next().filter((t) => folders.some((f) => board.inSubtree(t.folder_id, f)));
