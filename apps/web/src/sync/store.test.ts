import { v7 as uuidv7 } from "uuid";
import { expect, test } from "vitest";
import type { ErrorBody } from "../api/types/ErrorBody";
import type { LogEntry } from "../api/types/LogEntry";
import type { Mutation } from "../api/types/Mutation";
import type { Task } from "../api/types/Task";
import { type Pending, type Saved, Store } from "./store";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;

function task(n: number, title: string, extra: Partial<Task> = {}): Task {
  return {
    id: id(n),
    number: n,
    folder_id: null,
    title,
    description: "",
    done_at: null,
    start: null,
    due: null,
    sort_key: `a${n}`,
    created_at: 0,
    updated_at: 0,
    created_via: null,
    updated_via: null,
    ...extra,
  };
}

function setup(saved: Saved = Store.empty()) {
  /** What saving waits on; resolved unless a test holds it back. */
  let written = Promise.resolve();
  const sent: Mutation[] = [];
  const rejected: [Mutation, ErrorBody][] = [];
  const arrived: LogEntry[][] = [];
  let last = saved;
  const outbox = new Map(saved.outbox.map((p) => [p.mutation.id, p]));
  const store = new Store(saved, {
    save(copy, queued: Pending[], settled) {
      for (const p of queued) outbox.set(p.mutation.id, p);
      for (const id of settled) outbox.delete(id);
      last = { ...copy, outbox: [...outbox.values()] };
      return written;
    },
    send: (m) => sent.push(m),
    rejected: (m, e) => rejected.push([m, e]),
    arrived: (e) => arrived.push(e),
  });
  const titles = () => store.getView().board.sort([...store.getView().board.snapshot.tasks]).map((t) => t.title);
  const hold = () => {
    let release!: () => void;
    written = new Promise((r) => (release = r));
    return release;
  };
  return { store, sent, rejected, arrived, titles, hold, saved: () => last };
}

/** Lets the saves made so far finish. */
const settle = () => new Promise((r) => setTimeout(r));

/** How the server echoes a change back. */
const echo = (m: Mutation, seq: number, effects: LogEntry["effects"]): LogEntry => ({ seq, id: m.id, actor: null, effects, created_at: 0 });

test("a change shows at once, is sent once saved, and leaves the queue when the server confirms it", async () => {
  const { store, sent, titles, hold, saved } = setup();
  const release = hold();
  const { effects } = store.commit([{ op: "task.create", id: id(1), title: "交电费", sort_key: "a1" }]);
  expect(titles()).toEqual(["交电费"]);
  expect(store.getView().pending).toBe(1);
  expect(saved().outbox).toHaveLength(1);
  // Not sent, nor sent again on reconnecting, until it is saved.
  await settle();
  expect([sent.length, store.unconfirmed.length]).toEqual([0, 0]);
  release();
  await settle();
  expect([sent.length, store.unconfirmed.length]).toEqual([1, 1]);

  const made = effects[0].after as Task;
  store.receive([echo(sent[0], 1, [{ kind: "task", before: null, after: { ...made, number: 7 } }])], 1);
  expect(store.getView().pending).toBe(0);
  expect(store.getView().board.task(id(1))!.number).toBe(7);
  expect([store.since, saved().seq, saved().base.tasks.length]).toEqual([1, 1, 1]);
});

test("changes from elsewhere merge under the ones still queued", () => {
  const { store, titles, arrived } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "写初稿")], dependencies: [], ai_jobs: [] } });
  store.commit([{ op: "task.update", id: id(1), title: "写终稿" }]);
  const other = { kind: "task" as const, before: null, after: task(2, "投稿") };
  store.receive([{ seq: 5, id: id(50), actor: "Claude Code", effects: [other], created_at: 0 }], 5);
  expect(titles()).toEqual(["写终稿", "投稿"]);
  expect(arrived.flat().map((e) => e.actor)).toEqual(["Claude Code"]);
});

test("a rejected change leaves the view and is reported", async () => {
  const { store, sent, rejected, titles } = setup();
  store.commit([{ op: "task.create", id: id(1), title: "一", sort_key: "a1" }]);
  await settle();
  store.reject(sent[0].id, { code: "conflict", message: "事项 ID 已存在" });
  expect(titles()).toEqual([]);
  expect(rejected.map(([, e]) => e.message)).toEqual(["事项 ID 已存在"]);
});

test("a change another tab heard turned down leaves the view without being reported again", () => {
  const { store, rejected, titles, saved } = setup();
  const { id: change } = store.commit([{ op: "task.create", id: id(1), title: "一", sort_key: "a1" }]);
  store.drop(change);
  expect([titles(), saved().outbox, rejected]).toEqual([[], [], []]);
});

test("a change that no longer applies waits for the server instead of breaking the view", () => {
  const { store, titles } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "旧")], dependencies: [], ai_jobs: [] } });
  const { id: change } = store.commit([{ op: "task.update", id: id(1), title: "新" }]);
  store.receive([{ seq: 1, id: id(60), actor: null, effects: [{ kind: "task", before: task(1, "旧"), after: null }], created_at: 0 }], 1);
  expect(titles()).toEqual([]);
  expect(store.getView().stuck.has(change)).toBe(true);
});

test("changes the server has not seen yet can be undone here", async () => {
  const { store, titles } = setup();
  const { id: made } = store.commit([{ op: "task.create", id: id(1), title: "一", sort_key: "a1" }]);
  store.commit([{ op: "undo", mutation: made }]);
  expect(titles()).toEqual([]);
  await settle();
  expect(store.unconfirmed).toHaveLength(2);
});

test("a refused change is not queued", () => {
  const { store, sent } = setup();
  expect(() => store.commit([{ op: "task.update", id: id(9), title: "x" }])).toThrow("事项不存在");
  expect([store.unconfirmed.length, sent.length]).toEqual([0, 0]);
});

test("a fresh copy keeps the queued changes on top", () => {
  const { store, titles } = setup({ ...Store.empty(), seq: 3, base: { folders: [], tasks: [task(1, "旧")], dependencies: [], ai_jobs: [] } });
  store.commit([{ op: "task.create", id: id(2), title: "新", sort_key: "a2" }]);
  store.resync({ user: "u", seq: 90, folders: [], tasks: [task(3, "服务器上的")], dependencies: [], ai_jobs: [] });
  expect(titles()).toEqual(["新", "服务器上的"]);
  expect(store.since).toBe(90);
});

test("a change the fresh copy already has leaves the queue when the server acknowledges it again", async () => {
  const { store, sent, titles } = setup();
  store.commit([{ op: "task.create", id: id(1), title: "交电费", sort_key: "a1" }]);
  await settle();
  // The server applied it at 5, but the reply was lost; later the copy was too old and downloaded afresh.
  store.resync({ user: "u", seq: 9, folders: [], tasks: [task(1, "交电费")], dependencies: [], ai_jobs: [] });
  expect(store.getView().stuck.has(sent[0].id)).toBe(true);
  store.acked(sent[0].id, 12);
  expect(store.unconfirmed).toHaveLength(1);
  store.acked(sent[0].id, 5);
  expect([store.unconfirmed.length, store.getView().stuck.size]).toEqual([0, 0]);
  expect(titles()).toEqual(["交电费"]);
});

test("a change comes after the queued ones also when the clock has gone back since they were made", async () => {
  // Queued while the clock was a minute ahead, before the page opened again.
  const at = Date.now() + 60_000;
  const queued: Pending = { mutation: { id: uuidv7({ msecs: at }), ops: [{ op: "task.create", id: id(1), title: "交电费", sort_key: "a1" }] }, at };
  const { store, saved } = setup({ ...Store.empty(), outbox: [queued] });
  store.commit([{ op: "task.update", id: id(1), title: "交水费" }]);
  await settle();
  const ids = saved().outbox.map((p) => p.mutation.id);
  expect(ids.toSorted()).toEqual(ids);
});
