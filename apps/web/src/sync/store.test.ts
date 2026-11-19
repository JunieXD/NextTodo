import { v7 as uuidv7 } from "uuid";
import { expect, onTestFinished, test, vi } from "vitest";
import type { ErrorBody } from "../api/types/ErrorBody";
import type { LogEntry } from "../api/types/LogEntry";
import type { Mutation } from "../api/types/Mutation";
import type { Task } from "../api/types/Task";
import { type Pending, type Saved, Store } from "./store";

/** The user whose data it is. */
const ME = "019a4b2c-0000-7000-8000-0000000000ee";

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
    assignees: [],
    created_by: ME,
    updated_by: ME,
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
  const uploaded = new Set<string>();
  const awaited: [string, boolean][] = [];
  let last = saved;
  const outbox = new Map(saved.outbox.map((p) => [p.mutation.id, p]));
  const store = new Store(ME, saved, {
    save(copy, queued: Pending[], settled) {
      for (const p of queued) outbox.set(p.mutation.id, p);
      for (const id of settled) outbox.delete(id);
      last = { ...copy, outbox: [...outbox.values()] };
      return written;
    },
    send: (m) => sent.push(m),
    uploaded: (m) => m.ops.every((op) => op.op !== "attachment.add" || uploaded.has(op.id)),
    awaiting(m, missing) {
      awaited.push([m.id, missing]);
      // As the session's files do: missing, they are not uploaded from then on.
      if (missing) for (const op of m.ops) if (op.op === "attachment.add") uploaded.delete(op.id);
    },
    rejected: (m, e) => rejected.push([m, e]),
    arrived: (e) => arrived.push(e),
    confirmed() {},
  });
  const titles = () => store.getView().board.sort([...store.getView().board.snapshot.tasks]).map((t) => t.title);
  const hold = () => {
    let release!: () => void;
    written = new Promise((r) => (release = r));
    return release;
  };
  return { store, sent, rejected, arrived, titles, hold, uploaded, awaited, saved: () => last };
}

/** Lets the saves made so far finish. */
const settle = () => new Promise((r) => setTimeout(r));

/** How the server echoes a change back. */
const echo = (m: Mutation, seq: number, effects: LogEntry["effects"]): LogEntry => ({ seq, id: m.id, user: ME, actor: null, effects, created_at: 0 });

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

test("changes the Android widget could not send join the queue in the order made, and are sent once saved", async () => {
  const { store, sent, hold, saved } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "交房租")], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  const mine = store.commit([{ op: "task.create", id: id(2), title: "买菜", sort_key: "a2" }]);
  await settle();
  const earlier = { id: uuidv7({ msecs: 1 }), ops: [{ op: "task.update" as const, id: id(1), done: true }] };
  const release = hold();
  store.adopt([earlier, sent[0]]);
  expect(store.getView().board.task(id(1))!.done_at).not.toBeNull();
  expect(saved().outbox.map((p) => p.mutation.id).sort()).toEqual([earlier.id, mine.id].sort());
  expect(store.unconfirmed.map((m) => m.id)).toEqual([mine.id]);
  release();
  await settle();
  expect(sent.map((m) => m.id)).toEqual([mine.id, earlier.id]);
});

test("changes from elsewhere merge under the ones still queued", () => {
  const { store, titles, arrived } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "写初稿")], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  store.commit([{ op: "task.update", id: id(1), title: "写终稿" }]);
  const other = { kind: "task" as const, before: null, after: task(2, "投稿") };
  store.receive([{ seq: 5, id: id(50), user: ME, actor: "Claude Code", effects: [other], created_at: 0 }], 5);
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
  const { store, titles } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "旧")], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  const { id: change } = store.commit([{ op: "task.update", id: id(1), title: "新" }]);
  store.receive([{ seq: 1, id: id(60), user: ME, actor: null, effects: [{ kind: "task", before: task(1, "旧"), after: null }], created_at: 0 }], 1);
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

test("an attachment the server has not yet is made by the last change adding it or bringing it back", () => {
  const { store } = setup({
    ...Store.empty(),
    base: { ...Store.empty().base, tasks: [task(1, "审价")], attachments: [{ id: id(5), task_id: id(1), job_id: null, name: "报价.txt", type: "text/plain", size: 5, created_at: 0 }] },
  });
  const add = { op: "attachment.add", id: id(6), name: "合同.pdf", type: "application/pdf", size: 3, task_id: id(1) } as const;
  expect(store.madeBy(id(5))).toBeUndefined();
  const { id: added } = store.commit([add]);
  expect(store.madeBy(id(6))).toBe(added);
  const { id: deleted } = store.commit([{ op: "attachment.delete", id: id(5) }]);
  const { id: back } = store.commit([{ op: "undo", mutation: deleted }]);
  expect(store.madeBy(id(5))).toBe(back);
});

test("the last time a repeating task was done is found among the changes, also after undoing it, one time after another", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const } };
  const { store } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "背单词", { due })], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  // Done once on another device, then here.
  const first = task(1, "背单词", { due: { ...due, date: "2026-11-11" }, completions: { count: 1, last: "2026-11-10" } });
  store.receive([{ seq: 1, id: id(70), user: ME, actor: null, effects: [{ kind: "task", before: task(1, "背单词", { due }), after: first }], created_at: 0 }], 1);
  const { id: second } = store.commit([{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  store.commit([{ op: "task.update", id: id(1), title: "背单词 30 个" }]);
  const count = () => store.getView().board.task(id(1))!.completions?.count;
  expect([count(), store.completedAt(id(1), 2)]).toEqual([2, second]);

  const { id: undone } = store.commit([{ op: "undo", mutation: second }]);
  expect([count(), store.completedAt(id(1), 1)]).toEqual([1, id(70)]);
  // Undoing the undo is the last time again.
  const { id: redone } = store.commit([{ op: "undo", mutation: undone }]);
  expect([count(), store.completedAt(id(1), 2)]).toEqual([2, redone]);
  // Brought back after being deleted, it was not done one more time.
  const { id: deleted } = store.commit([{ op: "task.delete", id: id(1) }]);
  store.commit([{ op: "undo", mutation: deleted }]);
  expect(store.completedAt(id(1), 2)).toBe(redone);
});

test("a time done is not taken back once the task was done again since", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const } };
  const { store } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "背单词", { due })], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  const { id: first } = store.commit([{ op: "task.update", id: id(1), done: true, today: "2026-11-10" }]);
  store.commit([{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  // As offered on a copy that had not heard of the second time yet.
  expect(() => store.commit([{ op: "undo", mutation: first }])).toThrow("又完成或撤销过");
  expect(store.getView().board.task(id(1))!.completions?.count).toBe(2);
});

test("a time done, taken back and done again that day, is not taken back by undoing the first", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const } };
  const { store } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "背单词", { due })], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  const done = { op: "task.update" as const, id: id(1), done: true, today: "2026-11-10" };
  const { id: first } = store.commit([done]);
  const { id: undone } = store.commit([{ op: "undo", mutation: first }]);
  store.commit([done]);
  // The same count on the same day as after the first, yet not the first.
  expect(() => store.commit([{ op: "undo", mutation: first }])).toThrow("又完成或撤销过");
  expect(() => store.commit([{ op: "undo", mutation: undone }])).toThrow("又完成或撤销过");
  // Nor by a change that does it again first.
  expect(() => store.commit([done, { op: "undo", mutation: first }])).toThrow("又完成或撤销过");
  expect(store.getView().board.task(id(1))!.completions?.count).toBe(1);
});

test("times done are taken back one after another, the last first", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const } };
  const { store } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "背单词", { due })], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  const { id: first } = store.commit([{ op: "task.update", id: id(1), done: true, today: "2026-11-10" }]);
  const { id: second } = store.commit([{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  store.commit([{ op: "undo", mutation: second }]);
  store.commit([{ op: "undo", mutation: first }]);
  expect(store.getView().board.task(id(1))!.completions).toBeUndefined();
});

test("a change that did a task twice is undone whole", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const } };
  const { store } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "背单词", { due })], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  const { id: both } = store.commit(
    ["2026-11-10", "2026-11-11"].map((today) => ({ op: "task.update" as const, id: id(1), done: true, today })),
  );
  store.commit([{ op: "undo", mutation: both }]);
  const t = store.getView().board.task(id(1))!;
  expect([t.completions, t.due?.date]).toEqual([undefined, "2026-11-10"]);
});

test("the last time is not offered to undo when made and done in one change", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const } };
  const { store } = setup();
  store.commit([
    { op: "task.create", id: id(1), title: "背单词", sort_key: "a1", due },
    { op: "task.update", id: id(1), done: true, today: "2026-11-10" },
  ]);
  expect([store.getView().board.task(id(1))!.completions?.count, store.completedAt(id(1), 1)]).toEqual([1, undefined]);
});

test("the last time is not offered to undo when done together with other tasks", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const } };
  const tasks = [task(1, "背单词", { due }), task(2, "跑步", { due })];
  const { store } = setup({ ...Store.empty(), base: { folders: [], tasks, dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  store.commit([1, 2].map((n) => ({ op: "task.update" as const, id: id(n), done: true, today: "2026-11-10" })));
  // Undoing both would take back the run done since as well.
  store.commit([{ op: "task.update", id: id(2), done: true, today: "2026-11-11" }]);
  expect(store.completedAt(id(1), 1)).toBeUndefined();
});

test("a refused change is not queued", () => {
  const { store, sent } = setup();
  expect(() => store.commit([{ op: "task.update", id: id(9), title: "x" }])).toThrow("事项不存在");
  expect([store.unconfirmed.length, sent.length]).toEqual([0, 0]);
});

test("a fresh copy keeps the queued changes on top", () => {
  const { store, titles } = setup({ ...Store.empty(), seq: 3, base: { folders: [], tasks: [task(1, "旧")], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  store.commit([{ op: "task.create", id: id(2), title: "新", sort_key: "a2" }]);
  store.resync({ user: "u", seq: 90, folders: [], tasks: [task(3, "服务器上的")], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] });
  expect(titles()).toEqual(["新", "服务器上的"]);
  expect(store.since).toBe(90);
});

test("a change the fresh copy already has leaves the queue when the server acknowledges it again", async () => {
  const { store, sent, titles } = setup();
  store.commit([{ op: "task.create", id: id(1), title: "交电费", sort_key: "a1" }]);
  await settle();
  // The server applied it at 5, but the reply was lost; later the copy was too old and downloaded afresh.
  store.resync({ user: "u", seq: 9, folders: [], tasks: [task(1, "交电费")], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] });
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

test("a task becomes ready as its start comes, the view moving on then and not before, and back with the clock", () => {
  vi.useFakeTimers({ now: new Date(2026, 10, 12, 19, 59, 30) });
  onTestFinished(() => void vi.useRealTimers());
  const saved = Store.empty();
  saved.base.tasks = [task(1, "抢票", { start: "2026-11-12", start_time: "20:00" })];
  const { store } = setup(saved);
  let moved = 0;
  store.subscribe(() => moved++);
  const status = () => store.getView().board.status(store.getView().board.task(id(1))!);
  store.tick();
  expect([status(), moved]).toEqual(["waiting", 0]);
  vi.setSystemTime(new Date(2026, 10, 12, 20, 0, 5));
  store.tick();
  expect([status(), moved]).toEqual(["ready", 1]);
  // The clock or the time zone turned back.
  vi.setSystemTime(new Date(2026, 10, 12, 19, 30));
  store.tick();
  expect([status(), moved]).toEqual(["waiting", 2]);
});

test("a change adding attachments goes once its files are uploaded, and those after it once the server has it", async () => {
  const { store, sent, uploaded, awaited } = setup();
  store.connected();
  store.commit([{ op: "task.create", id: id(1), title: "报销", sort_key: "a1" }]);
  const add = store.commit([{ op: "attachment.add", id: id(2), name: "发票.png", type: "image/png", size: 3, task_id: id(1) }]);
  store.commit([{ op: "attachment.update", id: id(2), name: "发票 1.png" }]);
  await settle();
  expect(sent.map((m) => m.id)).toHaveLength(1);
  expect(awaited.at(-1)).toEqual([add.id, false]);

  uploaded.add(id(2));
  store.flush();
  expect(sent.map((m) => m.id).at(-1)).toBe(add.id);
  expect(sent).toHaveLength(2);

  // Its file is not on the server after all: it waits to be uploaded again, and what comes after it
  // with it.
  store.reject(add.id, { code: "file_missing", message: "附件的文件不在服务器上，请重新上传" });
  expect(awaited.at(-1)).toEqual([add.id, true]);
  expect(store.getView().pending).toBe(3);
  store.flush();
  expect(sent).toHaveLength(2);
  uploaded.add(id(2));
  store.flush();
  expect(sent).toHaveLength(3);
  expect(sent.at(-1)!.id).toBe(add.id);

  store.receive([echo(sent[0], 1, []), echo(sent[2], 2, [])], 2);
  expect(sent).toHaveLength(4);
  expect(sent.at(-1)!.ops[0].op).toBe("attachment.update");
});

test("the last time of a repeat is what made the task done until it is reopened and done again", () => {
  const due = { date: "2026-11-10", repeat: { every: 1, unit: "day" as const, from: "due" as const, until: "2026-11-10" } };
  const { store } = setup({ ...Store.empty(), base: { folders: [], tasks: [task(1, "背单词", { due })], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] } });
  const { id: last } = store.commit([{ op: "task.update", id: id(1), done: true, today: "2026-11-10" }]);
  expect([store.completedAt(id(1), 1), store.doneBy(id(1))]).toEqual([last, last]);
  store.commit([{ op: "task.update", id: id(1), done: false }]);
  const { id: again } = store.commit([{ op: "task.update", id: id(1), done: true }]);
  // Done again as it is now, the last time is no longer why.
  expect([store.completedAt(id(1), 1), store.doneBy(id(1))]).toEqual([last, again]);
});
