// Keeps each user's local copy in IndexedDB, so the app opens with data even when offline.
//
// - `copies`, keyed by [user id, copy id]: what the server confirmed, as each session has it. A
//   session writes only its own, so open tabs never overwrite each other; the app opens from the
//   newest.
// - `outbox`, keyed by [user id, mutation id]: the changes made on this device that the server has
//   not confirmed yet, shared by all tabs. A change goes in when it is made, before it is sent, and
//   out when a tab hears the server's verdict, in the same transaction as that tab's copy, which then
//   includes it.

import { type IDBPDatabase, openDB } from "idb";
import type { MutationId } from "../api/types/MutationId";
import type { Copy, Pending, Saved } from "./store";

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
  }).catch((e) => {
    opened = undefined;
    throw e;
  }));

/** All of a user's records in either store: arrays sort after strings. */
const all = (userId: string) => IDBKeyRange.bound([userId], [userId, []]);

/** The user's data saved on this device: the newest copy, and the outbox in the order the changes
 * were made (mutation ids are UUIDv7). The newest copy becomes `copyId`, in place of the others; a
 * session still open in another tab saves its own again with its next change. */
export async function load(userId: string, copyId: string): Promise<Saved | undefined> {
  const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
  const copies = tx.objectStore("copies");
  const found: Copy[] = await copies.getAll(all(userId));
  if (!found.length) return undefined;
  const newest = found.reduce((a, b) => (b.seq > a.seq ? b : a));
  const outbox: Pending[] = await tx.objectStore("outbox").getAll(all(userId));
  await copies.delete(all(userId));
  await copies.put(newest, [userId, copyId]);
  await tx.done;
  return { ...newest, outbox };
}

export interface Saver {
  /** Saves the copy and the changes that joined and left the outbox. Writes go one at a time: one
   * waiting for its turn takes the latest copy and everything since the last write. Resolves once
   * written, or once a write has failed: then `failed` hears of it, and what is not written yet goes
   * with the next save. */
  save(copy: Copy, queued: Pending[], settled: MutationId[]): Promise<void>;
  /** Stops saving, once what was saved so far is written; a failed write gets one more try. */
  close(): Promise<void>;
}

export function saver(userId: string, copyId: string, failed: (e: unknown) => void): Saver {
  let copy: Copy | undefined;
  const queued = new Map<MutationId, Pending>();
  const settled = new Set<MutationId>();
  /** Resolves the saves gathered for the next write. */
  let gathered: { promise: Promise<void>; resolve(): void } | undefined;
  let writing: Promise<void> | null = null;
  let closed = false;

  /** Takes everything saved since the last write. */
  const take = (c: Copy) => {
    const batch = { copy: c, queued: [...queued.values()], settled: [...settled], gathered };
    copy = undefined;
    queued.clear();
    settled.clear();
    gathered = undefined;
    return batch;
  };

  const write = () =>
    (writing ??= (async () => {
      while (copy) {
        const batch = take(copy);
        try {
          const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
          const outbox = tx.objectStore("outbox");
          await Promise.all([
            tx.objectStore("copies").put(batch.copy, [userId, copyId]),
            ...batch.queued.map((p) => outbox.put(p, [userId, p.mutation.id])),
            ...batch.settled.map((id) => outbox.delete([userId, id])),
            tx.done,
          ]);
          batch.gathered?.resolve();
        } catch (e) {
          copy ??= batch.copy;
          for (const p of batch.queued) if (!settled.has(p.mutation.id)) queued.set(p.mutation.id, p);
          for (const id of batch.settled) settled.add(id);
          failed(e);
          // Saves waiting for the next write hear of the failure too, instead of waiting for a save
          // that may not come; in the order they were made, which is the order changes go out in.
          batch.gathered?.resolve();
          gathered?.resolve();
          gathered = undefined;
          break;
        }
      }
      writing = null;
    })());

  return {
    save(c, joined, left) {
      if (closed) return Promise.resolve();
      copy = c;
      for (const p of joined) queued.set(p.mutation.id, p);
      for (const id of left) {
        queued.delete(id);
        settled.add(id);
      }
      if (!gathered) {
        let resolve!: () => void;
        gathered = { promise: new Promise((r) => (resolve = r)), resolve };
      }
      const done = gathered.promise;
      void write();
      return done;
    },
    async close() {
      closed = true;
      await writing;
      if (copy) await write();
    },
  };
}

/** Deletes everything of the user's on this device. Close the saver first, or a write may land
 * afterwards. */
export async function forget(userId: string) {
  const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
  await Promise.all([tx.objectStore("copies").delete(all(userId)), tx.objectStore("outbox").delete(all(userId)), tx.done]);
}

/** Deletes everyone's data on this device but the user's: arrays sort after strings, so their keys
 * lie below `[userId]` or above `[userId, []]`. */
export async function forgetOthers(userId: string) {
  const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
  const [copies, outbox] = [tx.objectStore("copies"), tx.objectStore("outbox")];
  const others = [IDBKeyRange.upperBound([userId], true), IDBKeyRange.lowerBound([userId, []], true)];
  await Promise.all([...others.flatMap((r) => [copies.delete(r), outbox.delete(r)]), tx.done]);
}

/** Deletes one session's copy of the user's data. */
export async function forgetCopy(userId: string, copyId: string) {
  await (await db()).delete("copies", [userId, copyId]);
}
