// Keeps each user's local copy in IndexedDB, so the app opens with data even when offline. Every
// open tab saves a copy of its own, so that tabs never overwrite each other's unsent changes; opening
// the app merges them into one. A change can stay unsent in a copy after the server has it, when its
// tab closed before hearing back: every save takes the changes its copy has received out of the
// others, and merging leaves out those the newest copy has received.

import { openDB } from "idb";
import type { Saved } from "./store";

/** This tab, for as long as the page is loaded. */
const TAB = crypto.randomUUID();

const db = () =>
  openDB("nexttodo", 1, {
    upgrade(db) {
      // Keyed by [user id, tab].
      db.createObjectStore("copies");
    },
  });

/** All the copies of a user's data: arrays sort after strings. */
const copies = (userId: string) => IDBKeyRange.bound([userId], [userId, []]);

/** The user's data saved on this device: the newest base, with the unsent changes of every copy in
 * the order they were made (mutation ids are UUIDv7), less those the base is known to include. It
 * becomes this tab's copy, replacing the others; a tab still open saves its own again with its next
 * change, and a change sent twice is applied once. */
export async function load(userId: string): Promise<Saved | undefined> {
  const tx = (await db()).transaction("copies", "readwrite");
  const all: Saved[] = await tx.store.getAll(copies(userId));
  if (!all.length) return undefined;
  const newest = all.reduce((a, b) => (b.seq > a.seq ? b : a));
  const included = new Set(newest.log.map(([id]) => id));
  const outbox = new Map(all.flatMap((c) => c.outbox).filter((p) => !included.has(p.mutation.id)).map((p) => [p.mutation.id, p]));
  const merged = { ...newest, outbox: [...outbox.values()].sort((a, b) => (a.mutation.id < b.mutation.id ? -1 : 1)) };
  await tx.store.delete(copies(userId));
  await tx.store.put(merged, [userId, TAB]);
  await tx.done;
  return merged;
}

export interface Saver {
  /** Saves the latest state, writing at most one at a time and skipping states that a newer one
   * replaced while a write was going on. The changes it has received leave the other copies. */
  save(s: Saved): void;
  /** Stops saving, once the write going on has finished. */
  close(): Promise<void>;
}

export function saver(userId: string): Saver {
  let next: Saved | undefined;
  let writing: Promise<void> | null = null;
  let closed = false;
  return {
    save(s) {
      if (closed) return;
      next = s;
      writing ??= (async () => {
        try {
          while (next) {
            const s = next;
            next = undefined;
            const tx = (await db()).transaction("copies", "readwrite");
            const received = new Set(s.log.map(([id]) => id));
            for (const key of await tx.store.getAllKeys(copies(userId))) {
              if ((key as string[])[1] === TAB) continue;
              const other: Saved = await tx.store.get(key);
              const outbox = other.outbox.filter((p) => !received.has(p.mutation.id));
              if (outbox.length < other.outbox.length) await tx.store.put({ ...other, outbox }, key);
            }
            await tx.store.put(s, [userId, TAB]);
            await tx.done;
          }
        } finally {
          writing = null;
        }
      })();
    },
    async close() {
      closed = true;
      next = undefined;
      // A write that failed has nothing left to wait for.
      await writing?.catch(() => {});
    },
  };
}

/** Deletes every copy of the user's data. Close the saver first, or a write may land afterwards. */
export async function forget(userId: string) {
  await (await db()).delete("copies", copies(userId));
}

/** Deletes this tab's copy of the user's data. */
export async function forgetHere(userId: string) {
  await (await db()).delete("copies", [userId, TAB]);
}
