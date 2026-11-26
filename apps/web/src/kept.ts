// Settings kept on this device, which every window showing them follows as they change.

import { useSyncExternalStore } from "react";

const listeners = new Set<() => void>();

/** What is kept on this device under `key`, and how to keep another value there. */
export function useKept(key: string): [string | null, (v: string) => void] {
  const value = useSyncExternalStore(
    (l) => {
      // A window hears its own changes from `keep`, the others', the panel's say, by `storage`.
      listeners.add(l);
      addEventListener("storage", l);
      return () => {
        listeners.delete(l);
        removeEventListener("storage", l);
      };
    },
    () => localStorage.getItem(key),
  );
  const keep = (v: string) => {
    localStorage.setItem(key, v);
    for (const l of listeners) l();
  };
  return [value, keep];
}
