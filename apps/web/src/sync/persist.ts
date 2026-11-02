// Keeps each user's local copy in IndexedDB, so the app opens with data even when offline.
//
// - `copies`, keyed by [user id, tab]: what the server confirmed, as each open tab has it. A tab
//   writes only its own, so tabs never overwrite each other; the app opens from the newest.
// - `outbox`, keyed by [user id, mutation id]: the changes made on this device that the server has
//   not confirmed yet, shared by all tabs. A change goes in when it is made, and out as soon as any
//   tab hears the server's verdict, in the same transaction as that tab's copy, which then includes
//   it. So a change is in the newest copy or in the outbox, never in both, whichever tab closes when.

import { type IDBPDatabase, openDB } from "idb";
import type { MutationId } from "../api/types/MutationId";
import type { Copy, Pending, Saved } from "./store";

/** This tab, for as long as the page is loaded. */
const TAB = crypto.randomUUID();

let opened: Promise<IDBPDatabase> | undefined;
const db = () =>
  (opened ??= openDB("nexttodo", 1, {
    upgrade(db) {
      db.createObjectStore("copies");
      db.createObjectStore("outbox");
    },
    // The browser closed it, say because the site's data was cleared: open it again next time.
    terminated() {
      opened = undefined;
    },
  }));

/** All of a user's records in either store: arrays sort after strings. */
const all = (userId: string) => IDBKeyRange.bound([userId], [userId, []]);

/** The user's data saved on this device: the newest copy, and the outbox in the order the changes
 * were made (mutation ids are UUIDv7). The newest copy becomes this tab's, in place of the others;
 * a tab still open saves its own again with its next change. */
export async function load(userId: string): Promise<Saved | undefined> {
  const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
  const copies = tx.objectStore("copies");
  const found: Copy[] = await copies.getAll(all(userId));
  if (!found.length) return undefined;
  const newest = found.reduce((a, b) => (b.seq > a.seq ? b : a));
  const outbox: Pending[] = await tx.objectStore("outbox").getAll(all(userId));
  await copies.delete(all(userId));
  await copies.put(newest, [userId, TAB]);
  await tx.done;
  return { ...newest, outbox };
}

export interface Saver {
  /** Saves this tab's copy and the changes that joined and left the outbox, writing at most once at
   * a time: a write waiting for its turn takes the latest copy and everything since the last one. */
  save(copy: Copy, queued: Pending[], settled: MutationId[]): void;
  /** Stops saving, once what was saved so far is written. */
  close(): Promise<void>;
}

export function saver(userId: string): Saver {
  let copy: Copy | undefined;
  const queued = new Map<MutationId, Pending>();
  const settled = new Set<MutationId>();
  let writing: Promise<void> | null = null;
  let closed = false;
  return {
    save(c, joined, left) {
      if (closed) return;
      copy = c;
      for (const p of joined) queued.set(p.mutation.id, p);
      for (const id of left) {
        queued.delete(id);
        settled.add(id);
      }
      writing ??= (async () => {
        try {
          while (copy) {
            const batch = { copy, queued: [...queued.values()], settled: [...settled] };
            copy = undefined;
            queued.clear();
            settled.clear();
            try {
              const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
              const outbox = tx.objectStore("outbox");
              for (const p of batch.queued) outbox.put(p, [userId, p.mutation.id]);
              for (const id of batch.settled) outbox.delete([userId, id]);
              tx.objectStore("copies").put(batch.copy, [userId, TAB]);
              await tx.done;
            } catch (e) {
              // Not written: it goes with the next save.
              copy ??= batch.copy;
              for (const p of batch.queued) if (!settled.has(p.mutation.id)) queued.set(p.mutation.id, p);
              for (const id of batch.settled) settled.add(id);
              throw e;
            }
          }
        } finally {
          writing = null;
        }
      })();
    },
    async close() {
      closed = true;
      // A write that failed has nothing left to wait for.
      await writing?.catch(() => {});
    },
  };
}

/** Deletes everything of the user's on this device. Close the saver first, or a write may land
 * afterwards. */
export async function forget(userId: string) {
  const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
  await Promise.all([tx.objectStore("copies").delete(all(userId)), tx.objectStore("outbox").delete(all(userId)), tx.done]);
}

/** Deletes this tab's copy of the user's data. */
export async function forgetHere(userId: string) {
  await (await db()).delete("copies", [userId, TAB]);
}
