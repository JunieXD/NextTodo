// Keeps each user's local copy in IndexedDB, so the app opens with data even when offline.

import { openDB } from "idb";
import type { Saved } from "./store";

const db = () =>
  openDB("nexttodo", 1, {
    upgrade(db) {
      db.createObjectStore("users");
    },
  });

export async function load(userId: string): Promise<Saved | undefined> {
  return (await db()).get("users", userId);
}

/** A function that saves the latest state, writing at most one at a time and skipping states that
 * a newer one replaced while a write was going on. */
export function saver(userId: string): (s: Saved) => void {
  let next: Saved | undefined;
  let writing = false;
  return async (s) => {
    next = s;
    if (writing) return;
    writing = true;
    try {
      while (next) {
        const s = next;
        next = undefined;
        await (await db()).put("users", s, userId);
      }
    } finally {
      writing = false;
    }
  };
}

export async function forget(userId: string) {
  await (await db()).delete("users", userId);
}
