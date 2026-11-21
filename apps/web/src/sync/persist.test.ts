import "fake-indexeddb/auto";
import { openDB } from "idb";
import { IDBFactory } from "fake-indexeddb";
import { expect, test, vi } from "vitest";
import { draft, keepDraft, stagedFiles, withdraw, withdrawn } from "./drafts";
import { forget, forgetCopy, forgetOthers, load, saver } from "./persist";
import { type Copy, Store } from "./store";

const user = (n: number) => `019a4b2c-0000-7000-8000-00000000000${n}`;
const someone = (id: string) => ({ id, username: "alice", display_name: "Alice", is_admin: false });
const pending = (n: number) => ({ mutation: { id: `019a4b2c-0000-7000-8000-00000000010${n}`, ops: [] }, at: n });
const copy = (seq: number): Copy => ({ ...Store.empty(), seq });
const fail = (e: unknown) => {
  throw e;
};

/** The database as another tab would see it; each test first lets `load` set it up. */
const raw = () => openDB("nexttodo", 3);
const mine = (u: string) => IDBKeyRange.bound([u], [u, []]);
const outbox = async (u: string) => (await (await raw()).getAll("outbox", mine(u))).map((p) => p.at);

test("opening takes the newest copy and the whole outbox, in the order the changes were made", async () => {
  const u = user(1);
  expect(await load(someone(u), "here")).toBeUndefined();
  const db = await raw();
  await db.put("copies", copy(5), [u, "tab-a"]);
  await db.put("copies", copy(9), [u, "tab-b"]);
  await db.put("copies", copy(1), [user(2), "tab-a"]);
  for (const n of [3, 1, 2]) await db.put("outbox", pending(n), [u, pending(n).mutation.id]);

  const opened = await load(someone(u), "here");
  expect(opened!.seq).toBe(9);
  expect(opened!.outbox.map((p) => p.at)).toEqual([1, 2, 3]);
  // The newest copy is this session's now, in place of the others.
  expect(await db.getAllKeys("copies", mine(u))).toEqual([[u, "here"]]);
  expect(await load(someone(user(2)), "there")).toBeDefined();
});

test("a change leaves the outbox with the copy that includes it, also one queued in another tab", async () => {
  const u = user(3);
  await load(someone(u), "here");
  // Another tab queued 1 and closed before hearing back.
  await (await raw()).put("outbox", pending(1), [u, pending(1).mutation.id]);
  const save = saver(u, "here", fail);
  void save.save(copy(1), [pending(2)], []);
  await save.save(copy(2), [pending(3)], [pending(1).mutation.id, pending(2).mutation.id]);
  expect(await outbox(u)).toEqual([3]);
  expect((await load(someone(u), "here"))!.seq).toBe(2);
});

test("a failed write is reported, and what it held goes with the next", async () => {
  const u = user(5);
  await load(someone(u), "here");
  const failed: unknown[] = [];
  const save = saver(u, "here", (e) => failed.push(e));
  // A function cannot be stored; the second save waits for the first write, and hears after it.
  const heard: number[] = [];
  await Promise.all([
    save.save({ ...copy(1), base: { ...copy(1).base, folders: [() => {}] as never } }, [pending(1)], []).then(() => heard.push(1)),
    save.save(copy(2), [pending(2)], []).then(() => heard.push(2)),
  ]);
  expect([failed.length, await outbox(u), heard]).toEqual([1, [], [1, 2]]);
  await save.save(copy(3), [pending(3)], []);
  expect([failed.length, await outbox(u)]).toEqual([1, [1, 2, 3]]);
});

test("a closed saver writes nothing more, and forgetting deletes what is left", async () => {
  const u = user(4);
  await load(someone(u), "here");
  const save = saver(u, "here", fail);
  void save.save(copy(10), [pending(1)], []);
  await save.close();
  await save.save(copy(11), [pending(2)], []);
  expect((await load(someone(u), "here"))!.seq).toBe(10);
  expect(await outbox(u)).toEqual([1]);

  await forgetCopy(u, "here");
  expect(await load(someone(u), "here")).toBeUndefined();
  expect(await outbox(u)).toEqual([1]);
  keepDraft(u, "next", "写了一半");
  await forget(u);
  expect([await outbox(u), draft(u, "next")]).toEqual([[], ""]);
});

test("signing in as someone deletes everyone else's data", async () => {
  const [a, b, c] = [user(6), user(7), user(8)];
  for (const u of [a, b, c]) {
    await load(someone(u), "here");
    const save = saver(u, "here", fail);
    await save.save(copy(1), [pending(1)], []);
    keepDraft(u, "next", u);
  }
  await forgetOthers(b);
  expect([a, b, c].map((u) => draft(u, "next"))).toEqual(["", b, ""]);
  expect([await load(someone(a), "here"), await load(someone(c), "here")]).toEqual([undefined, undefined]);
  expect([await outbox(a), await outbox(b), await outbox(c)]).toEqual([[], [1], []]);
  expect((await load(someone(b), "here"))!.seq).toBe(1);
});

test("a copy saved before sharing has the user in it, and every task as theirs", async () => {
  const u = user(9);
  await load(someone(u), "here");
  // As saved then: no shares, people, assignees or authors.
  const old = { ...copy(7), base: { folders: [], tasks: [{ id: "t", title: "旧" }], dependencies: [], ai_jobs: [] } };
  await (await raw()).put("copies", old, [u, "here"]);
  const { seq, base } = (await load(someone(u), "here"))!;
  expect([seq, base.shares, base.people, base.completions]).toEqual([7, [], [{ id: u, username: "alice", display_name: "Alice" }], []]);
  expect(base.tasks[0]).toMatchObject({ assignees: [], created_by: u, updated_by: u });
});

test("folders saved before they had colours and icons have the defaults, deleted ones too", async () => {
  const u = user(8);
  await load(someone(u), "here");
  const folder = { id: "f", parent_id: null, name: "旧", sort_key: "a0", created_at: 0, updated_at: 0 };
  const deleted = { ...folder, id: "g", name: "删掉的" };
  const deletion = pending(1).mutation.id;
  const old = { ...copy(3), base: { ...Store.empty().base, folders: [folder] }, log: [[deletion, [{ kind: "folder", before: deleted, after: null }]]] };
  await (await raw()).put("copies", old, [u, "here"]);
  const { base, log } = (await load(someone(u), "here"))!;
  expect(base.folders).toEqual([{ ...folder, color: null, icon: null }]);
  // Undoing the deletion brings it back as folders are now.
  expect(log).toEqual([[deletion, [{ kind: "folder", before: { ...deleted, color: null, icon: null }, after: null }]]]);
});

test("a reminder saved before tasks could have several is the one of a list, wherever a due date is", async () => {
  const u = user(0);
  await load(someone(u), "here");
  const old = { date: "2026-12-08", time: "18:00", remind_before: 60 };
  const now = { date: "2026-12-08", time: "18:00", reminders: [{ before: 60 }] };
  const task = { id: "t", title: "还花呗", due: old };
  const edit = { task: "t", due: old, after: [], reason: "", was: { title: "还花呗", description: "", due: old } };
  const create = { create: "n", title: "核算", due: old, after: [], before: [], reason: "" };
  const job = { id: "j", suggestions: [edit, create] };
  const m = pending(1).mutation.id;
  const saved = {
    ...copy(4),
    base: { ...Store.empty().base, tasks: [task], ai_jobs: [job] },
    log: [[m, [{ kind: "task", before: task, after: { ...task, due: { date: "2026-12-08" } } }]]],
  };
  const db = await raw();
  await db.put("copies", saved, [u, "here"]);
  const change = { mutation: { id: m, ops: [{ op: "task.update", id: "t", due: old }] }, at: 1 };
  await db.put("outbox", change, [u, m]);
  const { base, log, outbox } = (await load(someone(u), "here"))!;
  expect(base.tasks[0].due).toEqual(now);
  expect(base.ai_jobs[0].suggestions).toEqual([{ ...edit, due: now, was: { ...edit.was, due: now } }, { ...create, due: now }]);
  // Undoing the change here brings the reminder back.
  expect(log[0][1][0]).toEqual({ kind: "task", before: { ...task, due: now }, after: { ...task, due: { date: "2026-12-08" } } });
  expect(outbox[0].mutation.ops).toEqual([{ op: "task.update", id: "t", due: now }]);
});

test("a change withdrawn into a draft leaves the outbox as the app opens, its text and files back in the box once", async () => {
  const u = user(7);
  await load(someone(u), "here");
  const db = await raw();
  await db.put("copies", copy(1), [u, "tab-a"]);
  for (const n of [1, 2]) await db.put("outbox", pending(n), [u, pending(n).mutation.id]);
  // The tab the server turned the files down in wrote the draft, and closed before taking the change
  // out; told again, it puts nothing back twice.
  const file = { id: "019a4b2c-0000-7000-8000-000000000200", name: "图片 1.png", type: "image/png", size: 3, error: "文件超过服务器允许的大小" };
  const back = { place: "root", text: "买菜", files: [file] };
  withdraw(u, pending(1).mutation.id, back);
  withdraw(u, pending(1).mutation.id, back);
  const opened = await load(someone(u), "here");
  expect(opened!.outbox.map((p) => p.at)).toEqual([2]);
  // Still noted: a tab open meanwhile may have it, and is told by the tab that uploads.
  expect([draft(u, "root"), stagedFiles(u, "root"), withdrawn(u)]).toEqual(["买菜", [file], [pending(1).mutation.id]]);
});

test("the copies and outbox of the version released before attachments stay as it upgrades", async () => {
  vi.stubGlobal("indexedDB", new IDBFactory());
  vi.resetModules();
  const old = await openDB("nexttodo", 1, {
    upgrade(db) {
      db.createObjectStore("copies");
      db.createObjectStore("outbox");
    },
  });
  const u = user(9);
  await old.put("copies", copy(4), [u, "tab-a"]);
  await old.put("outbox", pending(1), [u, pending(1).mutation.id]);
  old.close();
  const opened = await (await import("./persist")).load(someone(u), "here");
  expect(opened!.seq).toBe(4);
  expect(opened!.outbox).toEqual([pending(1)]);
  vi.unstubAllGlobals();
});
