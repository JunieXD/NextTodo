// Uploading with several tabs open, when the tab doing it closes halfway: the others are not left
// waiting. The tabs share the device's storage and Web Locks; the one that closed is played here.

import "fake-indexeddb/auto";
import { openDB } from "idb";
import { expect, test, vi } from "vitest";
import type { Mutation } from "../api/types/Mutation";
import { withdraw, withdrawn } from "./drafts";
import { type FileMessage, Files } from "./files";
import { load, unqueue } from "./persist";
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

/** The device's storage for `u`, as the tab that closed left it, with the files it has kept. */
async function device(u: string, outbox: Mutation[], files: { id: string; uploaded: boolean; size?: number; shown?: number }[]) {
  await load(someone(u), "here");
  const db = await openDB("nexttodo", 3);
  for (const m of outbox) await db.put("outbox", { mutation: m, at: 0 }, [u, m.id]);
  for (const { id, uploaded, size = 3, shown = 0 } of files) {
    await db.put("files", new Blob(["png"]), [u, id]);
    await db.put("fileInfo", { size, uploaded, shown }, [u, id]);
  }
  return db;
}

/** A tab still open with `queued` in its outbox: what it sends and tells the others. */
function tab(u: string, queued: Mutation[]) {
  const sent: Mutation[] = [];
  const told: (FileMessage | { rejected: string })[] = [];
  const said: string[] = [];
  const store: Store = new Store(
    u,
    { ...Store.empty(), outbox: queued.map((mutation) => ({ mutation, at: 0 })) },
    {
      // As the session's saver does, of the outbox all tabs share.
      save: async (_, __, settled) => {
        for (const id of settled) await unqueue(u, id);
        return true;
      },
      send: (m) => sent.push(m),
      uploaded: (m) => files.has(m),
      awaiting: (m, missing) => files.awaiting(m, missing),
      rejected() {},
      arrived() {},
      confirmed: (ms) => files.confirmed(ms),
    },
  );
  const files: Files = new Files(u, store, { broadcast: (m) => told.push(m), told: (s) => said.push(s), signedOut() {} });
  return { store, files, sent, told, said };
}

test("a file uploaded by a tab that closed before telling: the tab waiting for it sends the change", async () => {
  const u = user(1);
  const [m, file] = [adding(1), id(6)];
  const db = await device(u, [m], [{ id: file, uploaded: false }]);
  // The other tab holds the lock, uploading; this one waits for it.
  let close = () => {};
  const closed = new Promise<void>((resolve) => (close = resolve));
  void navigator.locks.request(`upload:${u}`, () => closed);
  const open = tab(u, [m]);
  await vi.waitFor(async () => expect((await navigator.locks.query()).pending).toHaveLength(1));
  await db.put("fileInfo", { size: 3, uploaded: true, shown: 0 }, [u, file]);
  close();
  await vi.waitFor(() => expect(open.sent).toEqual([m]));
  open.files.stop();
});

test("a change turned down by a tab that closed before telling: the tab still holding it drops it, and goes on", async () => {
  const u = user(2);
  const [m, after] = [adding(1), { id: id(2), ops: [] }];
  // Noted as withdrawn and out of the outbox, but this tab was not told.
  await device(u, [after], [{ id: id(6), uploaded: false }]);
  withdraw(u, m.id);
  const open = tab(u, [m, after]);
  await vi.waitFor(() => expect(open.sent).toEqual([after]));
  expect(open.store.queued).toEqual([after]);
  expect(open.told).toEqual([{ rejected: m.id }]);
  expect(withdrawn(u)).toEqual([]);
  open.files.stop();
});

test("a change whose file is no longer on the device goes, as one turned down, and those after it go on", async () => {
  const u = user(3);
  const [m, after] = [adding(1), { id: id(2), ops: [] }];
  await device(u, [m, after], []);
  const open = tab(u, [m, after]);
  await vi.waitFor(() => expect(open.sent).toEqual([after]));
  // With why, for another tab the user may be in.
  expect(open.told).toEqual([{ rejected: m.id, told: { user: u, text: "“图片 1.png”上传失败：文件已不在这台设备上" } }]);
  open.files.stop();
});

test("a file a server from before attachments has nowhere to take is turned down, and the changes after it go", async () => {
  const u = user(4);
  const [m, after] = [adding(1), { id: id(2), ops: [] }];
  await device(u, [m, after], [{ id: id(6), uploaded: false }]);
  vi.stubGlobal("fetch", () => Promise.resolve(new Response(null, { status: 404 })));
  const open = tab(u, [m, after]);
  await vi.waitFor(() => expect(open.sent).toEqual([after]));
  expect(open.said).toEqual(["“图片 1.png”上传失败：服务器的版本太旧，还不支持附件，请先升级服务器"]);
  open.files.stop();
  vi.stubGlobal("fetch", () => Promise.reject(new Error("no server in these tests")));
});

test("files kept over the limit for changes still to be confirmed make room once they are", async () => {
  const u = user(4);
  const m = adding(1);
  const MB = 1024 * 1024;
  // Uploaded, the change waiting to be confirmed, alone over the limit; and a picture downloaded
  // long ago, which goes as the tab opens.
  const db = await device(u, [m], [
    { id: id(6), uploaded: true, size: 510 * MB, shown: 2 },
    { id: id(8), uploaded: true, size: 60 * MB, shown: 1 },
  ]);
  const kept = (store: string) => db.getAllKeys(store, IDBKeyRange.bound([u], [u, []]));
  const open = tab(u, [m]);
  await vi.waitFor(() => expect(open.sent).toEqual([m]));
  await vi.waitFor(async () => expect(await kept("fileInfo")).toEqual([[u, id(6)]]));
  open.store.acked(m.id, 0);
  await vi.waitFor(async () => expect(await kept("fileInfo")).toEqual([]));
  expect(await kept("files")).toEqual([]);
  open.files.stop();
});

test("a copy of a file waits for the change adding the file it copies, and leaves nothing kept once confirmed", async () => {
  const u = user(5);
  const [original, copy] = [adding(1), adding(2)];
  const db = await device(u, [original, copy], [{ id: id(6), uploaded: false }]);
  await db.put("fileInfo", { size: 0, uploaded: false, shown: 0, from: id(6), after: original.id }, [u, id(7)]);
  const asked: string[] = [];
  vi.stubGlobal("fetch", (url: string) => (asked.push(url.replace(/.*\/files\//, "")), Promise.resolve(new Response('{"size":3}'))));
  const open = tab(u, [original, copy]);
  await vi.waitFor(() => expect(open.sent).toEqual([original]));
  // Done uploading for now: copied before the server has the file it copies, the copy would be
  // turned down.
  await vi.waitFor(async () => expect((await navigator.locks.query()).held).toEqual([]));
  expect(asked).toEqual([id(6)]);
  open.store.acked(original.id, 0);
  await vi.waitFor(() => expect(open.sent).toEqual([original, copy]));
  expect(asked).toEqual([id(6), `${id(7)}?from=${id(6)}`]);
  open.store.acked(copy.id, 0);
  await vi.waitFor(async () => expect(await db.get("fileInfo", [u, id(7)])).toBeUndefined());
  // A file added after goes as any other.
  const more = adding(3);
  await db.put("outbox", { mutation: more, at: 0 }, [u, more.id]);
  await db.put("files", new Blob(["png"]), [u, id(8)]);
  await db.put("fileInfo", { size: 3, uploaded: false, shown: 0 }, [u, id(8)]);
  open.store.adopt([more]);
  await vi.waitFor(() => expect(open.sent).toEqual([original, copy, more]));
  open.files.stop();
  vi.stubGlobal("fetch", () => Promise.reject(new Error("no server in these tests")));
});

test("a copy of a file the server has goes at once, ahead of a change a tab left that closed before sending it", async () => {
  const u = user(6);
  // Made in the tab that closed, after the one still open had read the outbox: none sends it.
  const left: Mutation = { id: id(1), ops: [{ op: "folder.create", id: id(8), name: "收件箱" }] };
  const copy = adding(2);
  const db = await device(u, [left, copy], []);
  await db.put("fileInfo", { size: 0, uploaded: false, shown: 0, from: id(4) }, [u, id(7)]);
  const asked: string[] = [];
  vi.stubGlobal("fetch", (url: string) => (asked.push(url.replace(/.*\/files\//, "")), Promise.resolve(new Response('{"size":3}'))));
  const open = tab(u, [copy]);
  await vi.waitFor(() => expect(open.sent).toEqual([copy]));
  expect(asked).toEqual([`${id(7)}?from=${id(4)}`]);
  open.files.stop();
  vi.stubGlobal("fetch", () => Promise.reject(new Error("no server in these tests")));
});

test("a copy waiting for a change a tab left that closed before sending it holds up no file after it", async () => {
  const u = user(7);
  // The tab that closed added a file, and copied it.
  const [original, copy, mine] = [adding(1), adding(2), adding(3)];
  const db = await device(u, [original, copy, mine], [{ id: id(6), uploaded: false }, { id: id(8), uploaded: false }]);
  await db.put("fileInfo", { size: 0, uploaded: false, shown: 0, from: id(6), after: original.id }, [u, id(7)]);
  const asked: string[] = [];
  vi.stubGlobal("fetch", (url: string) => (asked.push(url.replace(/.*\/files\//, "")), Promise.resolve(new Response('{"size":3}'))));
  const locks = vi.spyOn(navigator.locks, "request");
  const open = tab(u, [mine]);
  await vi.waitFor(() => expect(open.sent).toEqual([mine]));
  expect(asked).toEqual([id(6), id(8)]);
  // Nor one added once the copy is all that waits, the server having confirmed the rest.
  const asks = locks.mock.calls.length;
  open.store.acked(mine.id, 0);
  await vi.waitFor(async () => {
    expect(locks.mock.calls.length).toBeGreaterThan(asks);
    const { held, pending } = await navigator.locks.query();
    expect([held, pending]).toEqual([[], []]);
  });
  const more = adding(4);
  await db.put("outbox", { mutation: more, at: 0 }, [u, more.id]);
  await db.put("files", new Blob(["png"]), [u, id(9)]);
  await db.put("fileInfo", { size: 3, uploaded: false, shown: 0 }, [u, id(9)]);
  open.store.adopt([more]);
  // Long before the copy is looked at again.
  await vi.waitFor(() => expect(open.sent).toEqual([mine, more]), { timeout: 500 });
  expect(asked).toEqual([id(6), id(8), id(9)]);
  locks.mockRestore();
  open.files.stop();
  vi.stubGlobal("fetch", () => Promise.reject(new Error("no server in these tests")));
});
