// Uploading with several tabs open, when the tab doing it closes halfway: the others are not left
// waiting. The tabs share the device's storage and Web Locks; the one that closed is played here.

import "fake-indexeddb/auto";
import { openDB } from "idb";
import { expect, test, vi } from "vitest";
import type { Mutation } from "../api/types/Mutation";
import { withdraw, withdrawn } from "./drafts";
import { type FileMessage, Files } from "./files";
import { load } from "./persist";
import { Store } from "./store";

vi.stubGlobal("document", { visibilityState: "visible", addEventListener() {}, removeEventListener() {} });
vi.stubGlobal("addEventListener", () => {});
vi.stubGlobal("removeEventListener", () => {});
// Nothing is uploaded here: the file is on the server already, or turned down.
vi.stubGlobal("fetch", () => Promise.reject(new Error("no server in these tests")));

const user = (n: number) => `019a4b2c-0000-7000-8000-00000000003${n}`;
const id = (n: number) => `019a4b2c-0000-7000-8000-00000000040${n}`;
const someone = (u: string) => ({ id: u, username: "alice", display_name: "Alice", is_admin: false });

/** Adds the file `n` to a task. */
const adding = (n: number): Mutation => ({
  id: id(n),
  ops: [{ op: "attachment.add", id: id(n + 5), name: "图片 1.png", type: "image/png", size: 3, task_id: id(9) }],
});

/** The device's storage for `u`, as the tab that closed left it. */
async function device(u: string, outbox: Mutation[], file: { id: string; uploaded: boolean }) {
  await load(someone(u), "here");
  const db = await openDB("nexttodo", 2);
  for (const m of outbox) await db.put("outbox", { mutation: m, at: 0 }, [u, m.id]);
  await db.put("files", { blob: new Blob(["png"]), uploaded: file.uploaded, shown: 0 }, [u, file.id]);
  return db;
}

/** A tab still open with `queued` in its outbox: what it sends and tells the others. */
function tab(u: string, queued: Mutation[]) {
  const sent: Mutation[] = [];
  const told: (FileMessage | { rejected: string })[] = [];
  const store: Store = new Store(
    u,
    { ...Store.empty(), outbox: queued.map((mutation) => ({ mutation, at: 0 })) },
    {
      save: async () => {},
      send: (m) => sent.push(m),
      uploaded: (m) => files.has(m),
      awaiting: (m, missing) => files.awaiting(m, missing),
      rejected() {},
      arrived() {},
    },
  );
  const files: Files = new Files(u, store, { broadcast: (m) => told.push(m), told() {}, signedOut() {} });
  return { store, files, sent, told };
}

test("a file uploaded by a tab that closed before telling: the tab waiting for it sends the change", async () => {
  const u = user(1);
  const [m, file] = [adding(1), id(6)];
  const db = await device(u, [m], { id: file, uploaded: false });
  // The other tab holds the lock, uploading; this one waits for it.
  let close = () => {};
  const closed = new Promise<void>((resolve) => (close = resolve));
  void navigator.locks.request(`upload:${u}`, () => closed);
  const open = tab(u, [m]);
  await vi.waitFor(async () => expect((await navigator.locks.query()).pending).toHaveLength(1));
  await db.put("files", { blob: new Blob(["png"]), uploaded: true, shown: 0 }, [u, file]);
  close();
  await vi.waitFor(() => expect(open.sent).toEqual([m]));
  open.files.stop();
});

test("a change turned down by a tab that closed before telling: the tab still holding it drops it, and goes on", async () => {
  const u = user(2);
  const [m, after] = [adding(1), { id: id(2), ops: [] }];
  // Noted as withdrawn and out of the outbox, but this tab was not told.
  await device(u, [after], { id: id(6), uploaded: false });
  withdraw(u, m.id);
  const open = tab(u, [m, after]);
  await vi.waitFor(() => expect(open.sent).toEqual([after]));
  expect(open.store.queued).toEqual([after]);
  expect(open.told).toEqual([{ rejected: m.id }]);
  expect(withdrawn(u)).toEqual([]);
  open.files.stop();
});
