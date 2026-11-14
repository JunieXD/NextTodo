// How many of a folder's most recently completed tasks stay in its list, struck through, instead of
// folding into its “已完成” (docs/ui.md#设置). Each device keeps its own, as it does the theme.

import { useSyncExternalStore } from "react";

const KEY = "nexttodo.keepDone";
export const MAX_KEEP_DONE = 20;
const listeners = new Set<() => void>();

const keepDone = () => Number(localStorage.getItem(KEY) ?? 0);

export function setKeepDone(n: number) {
  localStorage.setItem(KEY, String(n));
  for (const l of listeners) l();
}

export const useKeepDone = () =>
  useSyncExternalStore((l) => {
    listeners.add(l);
    return () => listeners.delete(l);
  }, keepDone);
