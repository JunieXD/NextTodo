import "fake-indexeddb/auto";
import { openDB } from "idb";
import { expect, test } from "vitest";
import { forget, forgetCopy, load, saver } from "./persist";
import { type Copy, Store } from "./store";

const user = (n: number) => `019a4b2c-0000-7000-8000-00000000000${n}`;
const pending = (n: number) => ({ mutation: { id: `019a4b2c-0000-7000-8000-00000000010${n}`, ops: [] }, at: n });
const copy = (seq: number): Copy => ({ ...Store.empty(), seq });
const fail = (e: unknown) => {
  throw e;
};

/** The database as another tab would see it; each test first lets `load` set it up. */
const raw = () => openDB("nexttodo", 1);
const mine = (u: string) => IDBKeyRange.bound([u], [u, []]);
const outbox = async (u: string) => (await (await raw()).getAll("outbox", mine(u))).map((p) => p.at);

test("opening takes the newest copy and the whole outbox, in the order the changes were made", async () => {
  const u = user(1);
  expect(await load(u, "here")).toBeUndefined();
  const db = await raw();
  await db.put("copies", copy(5), [u, "tab-a"]);
  await db.put("copies", copy(9), [u, "tab-b"]);
  await db.put("copies", copy(1), [user(2), "tab-a"]);
  for (const n of [3, 1, 2]) await db.put("outbox", pending(n), [u, pending(n).mutation.id]);

  const opened = await load(u, "here");
  expect(opened!.seq).toBe(9);
  expect(opened!.outbox.map((p) => p.at)).toEqual([1, 2, 3]);
  // The newest copy is this session's now, in place of the others.
  expect(await db.getAllKeys("copies", mine(u))).toEqual([[u, "here"]]);
  expect(await load(user(2), "there")).toBeDefined();
});

test("a change leaves the outbox with the copy that includes it, also one queued in another tab", async () => {
  const u = user(3);
  await load(u, "here");
  // Another tab queued 1 and closed before hearing back.
  await (await raw()).put("outbox", pending(1), [u, pending(1).mutation.id]);
  const save = saver(u, "here", fail);
  void save.save(copy(1), [pending(2)], []);
  await save.save(copy(2), [pending(3)], [pending(1).mutation.id, pending(2).mutation.id]);
  expect(await outbox(u)).toEqual([3]);
  expect((await load(u, "here"))!.seq).toBe(2);
});

test("a failed write is reported, and what it held goes with the next", async () => {
  const u = user(5);
  await load(u, "here");
  const failed: unknown[] = [];
  const save = saver(u, "here", (e) => failed.push(e));
  // A function cannot be stored; the second save waits for the first write.
  await Promise.all([save.save({ ...copy(1), base: { ...copy(1).base, folders: [() => {}] as never } }, [pending(1)], []), save.save(copy(2), [pending(2)], [])]);
  expect([failed.length, await outbox(u)]).toEqual([1, []]);
  await save.save(copy(3), [pending(3)], []);
  expect([failed.length, await outbox(u)]).toEqual([1, [1, 2, 3]]);
});

test("a closed saver writes nothing more, and forgetting deletes what is left", async () => {
  const u = user(4);
  await load(u, "here");
  const save = saver(u, "here", fail);
  void save.save(copy(10), [pending(1)], []);
  await save.close();
  await save.save(copy(11), [pending(2)], []);
  expect((await load(u, "here"))!.seq).toBe(10);
  expect(await outbox(u)).toEqual([1]);

  await forgetCopy(u, "here");
  expect(await load(u, "here")).toBeUndefined();
  expect(await outbox(u)).toEqual([1]);
  await forget(u);
  expect(await outbox(u)).toEqual([]);
});
