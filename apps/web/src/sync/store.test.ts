import { expect, test } from "vitest";
import type { ErrorBody } from "../api/types/ErrorBody";
import type { LogEntry } from "../api/types/LogEntry";
import type { Mutation } from "../api/types/Mutation";
import type { Task } from "../api/types/Task";
import { type Saved, Store } from "./store";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;

function task(n: number, title: string, extra: Partial<Task> = {}): Task {
  return {
    id: id(n),
    number: n,
    folder_id: null,
    title,
    description: "",
    done_at: null,
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
  const sent: Mutation[] = [];
  const rejected: [Mutation, ErrorBody][] = [];
  const arrived: LogEntry[][] = [];
  let last = saved;
  const store = new Store(saved, {
    save: (s) => (last = s),
    send: (m) => sent.push(m),
    rejected: (m, e) => rejected.push([m, e]),
    arrived: (e) => arrived.push(e),
  });
  const titles = () => store.getView().board.sort([...store.getView().board.snapshot.tasks]).map((t) => t.title);
  return { store, sent, rejected, arrived, titles, saved: () => last };
}

/** How the server echoes a change back. */
const echo = (m: Mutation, seq: number, effects: LogEntry["effects"]): LogEntry => ({ seq, id: m.id, actor: null, effects, created_at: 0 });

test("a change shows at once, is sent, and leaves the queue when the server confirms it", () => {
  const { store, sent, titles, saved } = setup();
  const { effects } = store.commit([{ op: "task.create", id: id(1), title: "交电费", sort_key: "a1" }]);
  expect(titles()).toEqual(["交电费"]);
  expect(store.getView().pending).toBe(1);
  expect(sent).toHaveLength(1);
  expect(saved().outbox).toHaveLength(1);

  const made = effects[0].after as Task;
  store.receive([echo(sent[0], 1, [{ kind: "task", before: null, after: { ...made, number: 7 } }])], 1);
  expect(store.getView().pending).toBe(0);
  expect(store.getView().board.task(id(1))!.number).toBe(7);
  expect([store.since, saved().seq, saved().base.tasks.length]).toEqual([1, 1, 1]);
});

test("changes from elsewhere merge under the ones still queued", () => {
  const { store, titles, arrived } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "写初稿")], dependencies: [] } });
  store.commit([{ op: "task.update", id: id(1), title: "写终稿" }]);
  const other = { kind: "task" as const, before: null, after: task(2, "投稿") };
  store.receive([{ seq: 5, id: id(50), actor: "Claude Code", effects: [other], created_at: 0 }], 5);
  expect(titles()).toEqual(["写终稿", "投稿"]);
  expect(arrived.flat().map((e) => e.actor)).toEqual(["Claude Code"]);
});

test("a rejected change leaves the view and is reported", () => {
  const { store, sent, rejected, titles } = setup();
  store.commit([{ op: "task.create", id: id(1), title: "一", sort_key: "a1" }]);
  store.reject(sent[0].id, { code: "conflict", message: "事项 ID 已存在" });
  expect(titles()).toEqual([]);
  expect(rejected.map(([, e]) => e.message)).toEqual(["事项 ID 已存在"]);
});

test("a change that no longer applies waits for the server instead of breaking the view", () => {
  const { store, titles } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "旧")], dependencies: [] } });
  const { id: change } = store.commit([{ op: "task.update", id: id(1), title: "新" }]);
  store.receive([{ seq: 1, id: id(60), actor: null, effects: [{ kind: "task", before: task(1, "旧"), after: null }], created_at: 0 }], 1);
  expect(titles()).toEqual([]);
  expect(store.getView().stuck.has(change)).toBe(true);
});

test("changes the server has not seen yet can be undone here", () => {
  const { store, titles } = setup();
  const { id: made } = store.commit([{ op: "task.create", id: id(1), title: "一", sort_key: "a1" }]);
  store.commit([{ op: "undo", mutation: made }]);
  expect(titles()).toEqual([]);
  expect(store.unconfirmed).toHaveLength(2);
});

test("a refused change is not queued", () => {
  const { store, sent } = setup();
  expect(() => store.commit([{ op: "task.update", id: id(9), title: "x" }])).toThrow("事项不存在");
  expect([store.unconfirmed.length, sent.length]).toEqual([0, 0]);
});

test("a fresh copy keeps the queued changes on top", () => {
  const { store, titles } = setup({ ...Store.empty(), seq: 3, base: { folders: [], tasks: [task(1, "旧")], dependencies: [] } });
  store.commit([{ op: "task.create", id: id(2), title: "新", sort_key: "a2" }]);
  store.resync({ seq: 90, folders: [], tasks: [task(3, "服务器上的")], dependencies: [] });
  expect(titles()).toEqual(["新", "服务器上的"]);
  expect(store.since).toBe(90);
});
