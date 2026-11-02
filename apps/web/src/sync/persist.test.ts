import "fake-indexeddb/auto";
import { openDB } from "idb";
import { expect, test } from "vitest";
import { forget, load, saver } from "./persist";
import { type Saved, Store } from "./store";

const USER = "019a4b2c-0000-7000-8000-000000000001";
const mutation = (n: number) => ({ mutation: { id: `019a4b2c-0000-7000-8000-00000000010${n}`, ops: [] }, at: n });
const copy = (seq: number, outbox: number[], log: number[] = []): Saved => ({
  ...Store.empty(),
  seq,
  outbox: outbox.map(mutation),
  log: log.map((n) => [mutation(n).mutation.id, []]),
});

test("opening merges what every tab saved: the newest base and the unsent changes it lacks, once each", async () => {
  expect(await load(USER)).toBeUndefined(); // which also sets the database up
  // What two other tabs left, written as they would have: the second one's base has the first
  // one's change 5 already.
  const db = await openDB("nexttodo", 1);
  await db.put("copies", copy(5, [1, 3, 5]), [USER, "tab-a"]);
  await db.put("copies", copy(9, [2, 3], [5]), [USER, "tab-b"]);
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

test("a save takes the changes it has received out of the other copies", async () => {
  const db = await openDB("nexttodo", 1);
  // Another tab sent 1 and 2, and closed before hearing back.
  await db.put("copies", copy(5, [1, 2]), [USER, "tab-a"]);
  const save = saver(USER);
  save.save(copy(9, [3], [1]));
  await save.close();
  expect((await db.get("copies", [USER, "tab-a"])).outbox.map((p: { at: number }) => p.at)).toEqual([2]);
  await forget(USER);
});
