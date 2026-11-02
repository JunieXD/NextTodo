import "fake-indexeddb/auto";
import { openDB } from "idb";
import { expect, test } from "vitest";
import { forget, load, saver } from "./persist";
import { type Saved, Store } from "./store";

const USER = "019a4b2c-0000-7000-8000-000000000001";
const mutation = (n: number) => ({ mutation: { id: `019a4b2c-0000-7000-8000-00000000010${n}`, ops: [] }, at: n });
const copy = (seq: number, outbox: number[]): Saved => ({ ...Store.empty(), seq, outbox: outbox.map(mutation) });

test("opening merges what every tab saved: the newest base and all unsent changes, once each", async () => {
  expect(await load(USER)).toBeUndefined(); // which also sets the database up
  // What two other tabs left, written as they would have.
  const db = await openDB("nexttodo", 2);
  await db.put("copies", copy(5, [1, 3]), [USER, "tab-a"]);
  await db.put("copies", copy(9, [2, 3]), [USER, "tab-b"]);
  await db.put("copies", copy(1, [4]), ["someone-else", "tab-a"]);

  const merged = await load(USER);
  expect(merged!.seq).toBe(9);
  expect(merged!.outbox.map((p) => p.at)).toEqual([1, 2, 3]);
  // It is this tab's copy now, in place of the others.
  expect(await db.getAll("copies", IDBKeyRange.bound([USER], [USER, []]))).toEqual([merged]);

  const save = saver(USER);
  save.save(copy(10, []));
  await save.close();
  save.save(copy(11, []));
  expect((await load(USER))!.seq).toBe(10);

  await forget(USER);
  expect(await load(USER)).toBeUndefined();
  expect(await load("someone-else")).toBeDefined();
});
