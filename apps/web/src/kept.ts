// Settings kept on this device, which every window showing them follows as they change.

import { useSyncExternalStore } from "react";

const listeners = new Set<(key: string | null) => void>();

/** Calls `l` with the key as anything kept on this device changes, here or in another window, `null`
 * when everything was cleared; returns how to stop. */
export function onKept(l: (key: string | null) => void): () => void {
  const other = (e: StorageEvent) => l(e.key);
  listeners.add(l);
  addEventListener("storage", other);
  return () => {
    listeners.delete(l);
    removeEventListener("storage", other);
  };
}

/** Keeps `v` under `key`, or nothing for `null`, telling this window; the others hear by `storage`. */
export function keep(key: string, v: string | null) {
  if (localStorage.getItem(key) === v) return;
  if (v === null) localStorage.removeItem(key);
  else localStorage.setItem(key, v);
  for (const l of listeners) l(key);
}

/** What is kept on this device under `key`, and how to keep another value there. */
export function useKept(key: string): [string | null, (v: string) => void] {
  const value = useSyncExternalStore(onKept, () => localStorage.getItem(key));
  return [value, (v) => keep(key, v)];
}
