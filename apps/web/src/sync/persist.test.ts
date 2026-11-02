import "fake-indexeddb/auto";
import { openDB } from "idb";
import { expect, test } from "vitest";
import { forget, forgetHere, load, saver } from "./persist";
import { type Copy, Store } from "./store";

const user = (n: number) => `019a4b2c-0000-7000-8000-00000000000${n}`;
const pending = (n: number) => ({ mutation: { id: `019a4b2c-0000-7000-8000-00000000010${n}`, ops: [] }, at: n });
const copy = (seq: number): Copy => ({ ...Store.empty(), seq });

/** The database as another tab would see it; each test first lets `load` set it up. */
const raw = () => openDB("nexttodo", 1);
const outbox = async (u: string) => (await (await raw()).getAll("outbox", IDBKeyRange.bound([u], [u, []]))).map((p) => p.at);

test("opening takes the newest copy and the whole outbox, in the order the changes were made", async () => {
  const u = user(1);
  expect(await load(u)).toBeUndefined();
  const db = await raw();
  await db.put("copies", copy(5), [u, "tab-a"]);
  await db.put("copies", copy(9), [u, "tab-b"]);
  await db.put("copies", copy(1), [user(2), "tab-a"]);
  for (const n of [3, 1, 2]) await db.put("outbox", pending(n), [u, pending(n).mutation.id]);

  const opened = await load(u);
  expect(opened!.seq).toBe(9);
  expect(opened!.outbox.map((p) => p.at)).toEqual([1, 2, 3]);
  // The newest copy is this tab's now, in place of the others.
  expect(await db.getAll("copies", IDBKeyRange.bound([u], [u, []]))).toEqual([copy(9)]);
  expect(await load(user(2))).toBeDefined();
});

test("a change leaves the outbox with the copy that includes it, also one queued in another tab", async () => {
  const u = user(3);
  await load(u);
  // Another tab queued 1 and closed before hearing back.
  await (await raw()).put("outbox", pending(1), [u, pending(1).mutation.id]);
  const save = saver(u);
  save.save(copy(1), [pending(2)], []);
  save.save(copy(2), [pending(3)], [pending(1).mutation.id, pending(2).mutation.id]);
  await save.close();
  expect(await outbox(u)).toEqual([3]);
  expect((await load(u))!.seq).toBe(2);
});

test("a closed saver writes nothing more, and forgetting deletes what is left", async () => {
  const u = user(4);
  await load(u);
  const save = saver(u);
  save.save(copy(10), [pending(1)], []);
  await save.close();
  save.save(copy(11), [pending(2)], []);
  expect((await load(u))!.seq).toBe(10);
  expect(await outbox(u)).toEqual([1]);

  await forgetHere(u);
  expect(await load(u)).toBeUndefined();
  expect(await outbox(u)).toEqual([1]);
  await forget(u);
  expect(await outbox(u)).toEqual([]);
});
