// Keeps each user's local copy in IndexedDB, so the app opens with data even when offline.
//
// - `copies`, keyed by [user id, copy id]: what the server confirmed, as each session has it. A
//   session writes only its own, so open tabs never overwrite each other; the app opens from the
//   newest.
// - `outbox`, keyed by [user id, mutation id]: the changes made on this device that the server has
//   not confirmed yet, shared by all tabs. A change goes in when it is made, before it is sent, and
//   out when a tab hears the server's verdict, in the same transaction as that tab's copy, which then
//   includes it.
//
// - `files` and `fileInfo`, keyed by [user id, attachment id]: attachments' files, and what is known
//   of them (`files.ts`).
//
// The drafts of the add boxes, and the AI settings kept on the device (ai.ts), are in localStorage
// and go along with the rest.

import { type IDBPDatabase, openDB } from "idb";
import type { AiJob } from "../api/types/AiJob";
import type { Due } from "../api/types/Due";
import type { Effect } from "../api/types/Effect";
import type { Folder } from "../api/types/Folder";
import type { MutationId } from "../api/types/MutationId";
import type { Suggestion } from "../api/types/Suggestion";
import type { Task } from "../api/types/Task";
import type { User } from "../api/types/User";
import type { Was } from "../api/types/Was";
import { forgetAiConfig, forgetOtherAiConfigs } from "../ai";
import { forgetDrafts, forgetOtherDrafts, withdrawn } from "./drafts";
import type { Copy, Pending, Saved } from "./store";

let opened: Promise<IDBPDatabase> | undefined;
export const db = () =>
  (opened ??= new Promise<IDBPDatabase>((resolve, reject) => {
    openDB("nexttodo", 3, {
      upgrade(db, from) {
        if (from < 1) {
          db.createObjectStore("copies");
          db.createObjectStore("outbox");
        }
        // Version 2, never released, kept a file's bytes in one record with what changes of it.
        if (from === 2) db.deleteObjectStore("files");
        db.createObjectStore("files");
        db.createObjectStore("fileInfo");
      },
      // A page of an earlier version of the app has it open and does not give way: only closing that
      // page lets this one upgrade it.
      blocked: () => reject(new Error("其他开着的 NextTodo 页面还是旧版本，请关闭它们后刷新这一页")),
      // A later version of the app is opening it in another tab: this page gives way, and loads as that
      // version.
      blocking(_, __, e) {
        (e.target as IDBDatabase).close();
        location.reload();
      },
      // The browser closed it, say because the site's data was cleared: open it again next time.
      terminated() {
        opened = undefined;
      },
    }).then(resolve, reject);
  }).catch((e) => {
    opened = undefined;
    throw e;
  }));

/** All of a user's records in a store: arrays sort after strings. */
export const all = (userId: string) => IDBKeyRange.bound([userId], [userId, []]);

/** Everyone's records but the user's: arrays sort after strings, so their keys lie below `[userId]`
 * or above `[userId, []]`. */
const others = (userId: string) => [IDBKeyRange.upperBound([userId], true), IDBKeyRange.lowerBound([userId, []], true)];

/** The user's data saved on this device: the newest copy, and the outbox in the order the changes
 * were made (mutation ids are UUIDv7). The newest copy becomes `copyId`, in place of the others; a
 * session still open in another tab saves its own again with its next change. Changes withdrawn for a
 * file turned down (docs/web.md#附件) leave the outbox first, if a tab closed before they did; they stay noted
 * as withdrawn for the tab that uploads to tell the other tabs. */
export async function load(user: User, copyId: string): Promise<Saved | undefined> {
  const gone = withdrawn(user.id);
  const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
  const copies = tx.objectStore("copies");
  const found: Copy[] = await copies.getAll(all(user.id));
  if (!found.length) return undefined;
  const newest = current(found.reduce((a, b) => (b.seq > a.seq ? b : a)), user);
  const outbox = tx.objectStore("outbox");
  for (const id of gone) await outbox.delete([user.id, id]);
  const pending: Pending[] = await outbox.getAll(all(user.id));
  await copies.delete(all(user.id));
  await copies.put(newest, [user.id, copyId]);
  await tx.done;
  return { ...newest, outbox: pending.map(pendingOf) };
}

/** The changes in the user's outbox, as every tab has saved them. */
export async function outbox(userId: string): Promise<Pending[]> {
  return (await db()).getAll("outbox", all(userId));
}

/** Takes a change out of the user's outbox, whichever tab made it. */
export async function unqueue(userId: string, id: MutationId) {
  await (await db()).delete("outbox", [userId, id]);
}

/** A due date as kept before a task could have several reminders: with one before its time, if any. */
const dueOf = ({ remind_before, ...due }: Due & { remind_before?: number }): Due =>
  remind_before === undefined ? due : { ...due, reminders: [{ before: remind_before }] };

const taskOf = (t: Task): Task => (t.due ? { ...t, due: dueOf(t.due) } : t);
const wasOf = (w: Was): Was => (w.due ? { ...w, due: dueOf(w.due) } : w);

function suggestionOf(s: Suggestion): Suggestion {
  if ("create" in s) return s.due ? { ...s, due: dueOf(s.due) } : s;
  if ("delete" in s) return { ...s, was: wasOf(s.was) };
  if ("task" in s) return { ...s, ...(s.due && { due: dueOf(s.due) }), was: wasOf(s.was) };
  return s;
}

const jobOf = (j: AiJob): AiJob => (j.suggestions ? { ...j, suggestions: j.suggestions.map(suggestionOf) } : j);

/** A change saved by an earlier version, as one is now: due dates as they are. */
const pendingOf = (p: Pending): Pending => ({
  ...p,
  mutation: { ...p.mutation, ops: p.mutation.ops.map((op) => ((op.op === "task.create" || op.op === "task.update") && op.due ? { ...op, due: dueOf(op.due) } : op)) },
});

/** A copy saved by an earlier version, as one is now. Before folders could be shared, nothing was,
 * every task was the user's, and the user was the only one in it; its log of changes goes, being of
 * tasks as they were then. Before AI organizing there were no jobs, before attachments none, before
 * completion records none, and before folders had colours and icons, none of those: neither for
 * folders kept nor for those its log of changes brings back when one is undone. Due dates, of tasks
 * and AI's suggestions, in the log too, are as they are now. */
function current(saved: Copy, { id, username, display_name }: User): Copy {
  const looked = (f: Folder): Folder => ({ ...f, color: f.color ?? null, icon: f.icon ?? null });
  const folders = saved.base.folders.map(looked);
  const log = saved.log.map(([m, effects]): [MutationId, Effect[]] => [
    m,
    effects.map((e) =>
      e.kind === "folder"
        ? { ...e, before: e.before && looked(e.before), after: e.after && looked(e.after) }
        : e.kind === "task"
          ? { ...e, before: e.before && taskOf(e.before), after: e.after && taskOf(e.after) }
          : e.kind === "ai_job"
            ? { ...e, before: e.before && jobOf(e.before), after: e.after && jobOf(e.after) }
            : e,
    ),
  ]);
  const base = { ...saved.base, folders, tasks: saved.base.tasks.map(taskOf), ai_jobs: saved.base.ai_jobs?.map(jobOf) };
  const c = { ...saved, base: { ...base, attachments: saved.base.attachments ?? [], completions: saved.base.completions ?? [] }, log };
  if (c.base.people) return c;
  const tasks = c.base.tasks.map((t) => ({ ...t, assignees: [], created_by: id, updated_by: id }));
  const people = [{ id, username, display_name }];
  return { ...c, base: { ...c.base, tasks, ai_jobs: c.base.ai_jobs ?? [], shares: [], people }, log: [] };
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
  forgetDrafts(userId);
  forgetAiConfig(userId);
  await forgetSynced(userId);
  const tx = (await db()).transaction(["files", "fileInfo"], "readwrite");
  await Promise.all([tx.objectStore("files").delete(all(userId)), tx.objectStore("fileInfo").delete(all(userId)), tx.done]);
}

/** Deletes the user's copies and outbox, which belong to the server's history of their data; the
 * drafts and the AI settings stay. */
export async function forgetSynced(userId: string) {
  const tx = (await db()).transaction(["copies", "outbox"], "readwrite");
  await Promise.all([tx.objectStore("copies").delete(all(userId)), tx.objectStore("outbox").delete(all(userId)), tx.done]);
}

/** How many changes wait on this device to be sent, everyone's. */
export async function unsent() {
  return (await db()).count("outbox");
}

/** How many changes of everyone but the user wait on this device to be sent. */
export async function unsentOfOthers(userId: string) {
  const outbox = (await db()).transaction("outbox").store;
  const [below, above] = await Promise.all(others(userId).map((r) => outbox.count(r)));
  return below + above;
}

/** Deletes everyone's data on this device but the user's. */
export async function forgetOthers(userId: string) {
  forgetOtherDrafts(userId);
  forgetOtherAiConfigs(userId);
  const names = ["copies", "outbox", "files", "fileInfo"];
  const tx = (await db()).transaction(names, "readwrite");
  const stores = names.map((s) => tx.objectStore(s));
  await Promise.all([...others(userId).flatMap((r) => stores.map((s) => s.delete(r))), tx.done]);
}

/** Deletes one session's copy of the user's data. */
export async function forgetCopy(userId: string, copyId: string) {
  await (await db()).delete("copies", [userId, copyId]);
}
