// Where the quick add bars that have a folder to pick put a task (docs/ui.md#其他): the folder last
// picked, kept on this device for each user and shared by all of them; or, fixed in the settings, going
// with the account (`prefs.ts`), that one, which picking another overrides until the next add.

import { useSyncExternalStore } from "react";
import type { FolderId } from "../api/types/FolderId";
import type { Board } from "../domain/board";
import { keep, onKept } from "../kept";
import { prefsOf } from "../prefs";

const PICKED = "nexttodo.addTo:";

const parse = (saved: string | null): FolderId | null | undefined => (saved === null ? undefined : JSON.parse(saved));

/** The folder fixed in the settings, still there or not; undefined when the bars go on from the one
 * last picked. */
export function fixedAddTo(board: Board): FolderId | null | undefined {
  const to = prefsOf(board).add_to;
  return to ? to.folder : undefined;
}

/** The folder picked, or else the one fixed, if it is still there; undefined for the bar's own default. */
export function useAddTo(board: Board): FolderId | null | undefined {
  const picked = parse(useSyncExternalStore(onKept, () => localStorage.getItem(PICKED + board.me)));
  return [picked, fixedAddTo(board)].find((f) => f !== undefined && board.hasFolder(f));
}

export const pickAddTo = (user: string, f: FolderId | null) => keep(PICKED + user, JSON.stringify(f));

/** As a folder is fixed: the bars go on from it, not from the one last picked. */
export const unpickAddTo = (user: string) => keep(PICKED + user, null);

/** After a bar has added to `used`: a folder picked over the fixed one was for that once. Not one
 * another window has picked since. */
export function addedTo(board: Board, used: FolderId | null) {
  if (fixedAddTo(board) !== undefined && localStorage.getItem(PICKED + board.me) === JSON.stringify(used)) unpickAddTo(board.me);
}
