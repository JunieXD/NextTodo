import { expect, test } from "vitest";
import type { Effect } from "../api/types/Effect";
import type { Op } from "../api/types/Op";
import { type Data, Rejected, apply, fromSnapshot, revert } from "./apply";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const log = new Map<string, Effect[]>();
const cx = { now: 1000, effectsOf: (m: string) => log.get(m) };

function empty(): Data {
  return fromSnapshot({ folders: [], tasks: [], dependencies: [], ai_jobs: [] });
}

/** Applies a mutation and remembers its effects under `mutation`, for undo. */
function run(d: Data, mutation: number, ops: Op[]): Effect[] {
  const effects = apply(d, ops, cx);
  log.set(id(mutation), effects);
  return effects;
}

const titles = (d: Data) => [...d.tasks.values()].map((t) => t.title);

test("new things go last, and names stay unique among siblings", () => {
  const d = empty();
  run(d, 100, [
    { op: "folder.create", id: id(1), name: "工作" },
    { op: "task.create", id: id(2), title: "一", folder_id: id(1) },
    { op: "task.create", id: id(3), title: "二", folder_id: id(1) },
  ]);
  const [a, b] = [d.tasks.get(id(2))!, d.tasks.get(id(3))!];
  expect(a.sort_key < b.sort_key).toBe(true);
  expect(a.number).toBe(0);
  expect(() => apply(d, [{ op: "folder.create", id: id(4), name: "工作" }], cx)).toThrow("同一层级已有名为“工作”的文件夹");
});

test("a failed mutation leaves nothing behind", () => {
  const d = empty();
  run(d, 101, [
    { op: "task.create", id: id(1), title: "设计" },
    { op: "task.create", id: id(2), title: "开发" },
    { op: "dep.add", before: id(1), after: id(2) },
  ]);
  const ops: Op[] = [
    { op: "task.create", id: id(3), title: "测试" },
    { op: "dep.add", before: id(2), after: id(1) },
  ];
  expect(() => apply(d, ops, cx)).toThrow(new Rejected("cycle", "添加该依赖将形成循环：“开发” → “设计” → “开发”"));
  expect(titles(d)).toEqual(["设计", "开发"]);
  expect(d.deps.size).toBe(1);
});

test("deleting a folder takes everything inside, and undo brings it back", () => {
  const d = empty();
  run(d, 102, [
    { op: "folder.create", id: id(1), name: "论文" },
    { op: "folder.create", id: id(2), name: "答辩", parent_id: id(1) },
    { op: "task.create", id: id(3), title: "PPT", folder_id: id(2) },
    { op: "task.create", id: id(4), title: "外面" },
    { op: "dep.add", before: id(4), after: id(3) },
  ]);
  const effects = run(d, 103, [{ op: "folder.delete", id: id(1) }]);
  expect(effects.map((e) => e.kind)).toEqual(["dependency", "task", "folder", "folder"]);
  expect(effects.slice(2).map((e) => e.before && "name" in e.before && e.before.name)).toEqual(["答辩", "论文"]);
  expect([d.folders.size, titles(d), d.deps.size]).toEqual([0, ["外面"], 0]);

  run(d, 104, [{ op: "undo", mutation: id(103) }]);
  expect([d.folders.size, titles(d).sort(), d.deps.size]).toEqual([2, ["PPT", "外面"], 1]);
});

test("undo puts back only the fields the change touched", () => {
  const d = empty();
  run(d, 105, [{ op: "task.create", id: id(1), title: "初稿" }]);
  run(d, 106, [{ op: "task.update", id: id(1), done: true }]);
  run(d, 107, [{ op: "task.update", id: id(1), title: "终稿" }]);
  run(d, 108, [{ op: "undo", mutation: id(106) }]);
  const t = d.tasks.get(id(1))!;
  expect([t.title, t.done_at]).toEqual(["终稿", null]);
  expect(() => apply(d, [{ op: "undo", mutation: id(999) }], cx)).toThrow("要撤销的改动不存在或已过期");
});

test("folders cannot move into themselves", () => {
  const d = empty();
  run(d, 109, [
    { op: "folder.create", id: id(1), name: "上" },
    { op: "folder.create", id: id(2), name: "下", parent_id: id(1) },
  ]);
  expect(() => apply(d, [{ op: "folder.update", id: id(1), parent_id: id(2) }], cx)).toThrow("文件夹不能移入自身或其子文件夹");
});

test("a repeating task moves on rather than being done, and is never done while it repeats", () => {
  const d = empty();
  const weekly = { every: 1, unit: "week", from: "due" } as const;
  run(d, 120, [{ op: "task.create", id: id(1), title: "开例会", due: { date: "2026-11-09", time: "10:00", repeat: weekly } }]);
  expect(() => apply(d, [{ op: "task.update", id: id(1), done: true }], cx)).toThrow("完成重复的事项需要给出今天的日期");
  run(d, 121, [{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  expect(d.tasks.get(id(1))).toMatchObject({ done_at: null, due: { date: "2026-11-16", time: "10:00", repeat: weekly }, completions: { count: 1, last: "2026-11-11" } });
  // Undone after the reminder changed, it moves back and the reminder stays as changed; it was never done.
  run(d, 122, [{ op: "task.update", id: id(1), due: { date: "2026-11-16", time: "10:00", remind_before: 30, repeat: weekly } }]);
  run(d, 123, [{ op: "undo", mutation: id(121) }]);
  expect(d.tasks.get(id(1))!.due).toEqual({ date: "2026-11-09", time: "10:00", remind_before: 30, repeat: weekly });
  expect(d.tasks.get(id(1))!.completions).toBeUndefined();
  // Undone after the due date was cleared, only the date comes back: the rest stays cleared.
  run(d, 124, [{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  run(d, 125, [{ op: "task.update", id: id(1), due: null }]);
  run(d, 126, [{ op: "undo", mutation: id(124) }]);
  expect(d.tasks.get(id(1))!.due).toEqual({ date: "2026-11-09" });
  // Repeating no more as it is done, it is done, which isn't one more time.
  run(d, 127, [{ op: "task.update", id: id(1), due: { date: "2026-11-09", repeat: weekly } }]);
  run(d, 128, [{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  run(d, 129, [{ op: "task.update", id: id(1), due: { date: "2026-11-16" }, done: true }]);
  expect(d.tasks.get(id(1))).toMatchObject({ due: { date: "2026-11-16" }, completions: { count: 1, last: "2026-11-11" } });
  expect(d.tasks.get(id(1))!.done_at).not.toBeNull();
  const doneRepeating: Op = { op: "task.create", id: id(2), title: "交房租", done: true, due: { date: "2026-11-30", repeat: weekly } };
  expect(() => apply(d, [doneRepeating], cx)).toThrow("“交房租”已完成，不能设置重复，请先重新打开");
});

test("revert undoes what replaying effects did", () => {
  const d = empty();
  const effects = run(d, 110, [
    { op: "task.create", id: id(1), title: "一" },
    { op: "task.update", id: id(1), title: "二" },
  ]);
  revert(d, effects);
  expect(d.tasks.size).toBe(0);
});

test("AI jobs: only a failed one is retried, they go with their folder, and one being organized comes back failed", () => {
  const d = empty();
  const job = (state: "waiting" | "running" | "failed") => ({ ...d.jobs.get(id(2))!, state, error: state === "failed" ? "接口超时" : null });
  run(d, 110, [
    { op: "folder.create", id: id(1), name: "读书" },
    { op: "ai_job.create", id: id(2), text: "读完第三章", local_time: "2026-11-04 16:30 星期三", folder_id: id(1) },
  ]);
  expect(d.jobs.get(id(2))!.state).toBe("waiting");
  expect(() => apply(d, [{ op: "ai_job.retry", id: id(2) }], cx)).toThrow("这条请求已在等待整理");

  // As the server has it by now: organizing, then failed.
  d.jobs.set(id(2), job("running"));
  expect(() => apply(d, [{ op: "ai_job.retry", id: id(2), text: "改" }], cx)).toThrow("正在整理，稍后再改");
  d.jobs.set(id(2), job("failed"));
  run(d, 111, [{ op: "ai_job.retry", id: id(2), text: "读完第四章", folder_id: null }]);
  expect(d.jobs.get(id(2))).toMatchObject({ state: "waiting", error: null, text: "读完第四章", folder_id: null });

  // Back in the folder and organizing: deleting the folder takes it, undoing brings it back failed.
  d.jobs.set(id(2), { ...job("running"), folder_id: id(1) });
  run(d, 113, [{ op: "folder.delete", id: id(1) }]);
  expect(d.jobs.size).toBe(0);
  run(d, 114, [{ op: "undo", mutation: id(113) }]);
  expect(d.jobs.get(id(2))).toMatchObject({ state: "failed", error: "已撤销", folder_id: id(1) });
});

test("AI jobs: a tidying with suggestions is tidied again from scratch, and comes back as it was once taken and undone", () => {
  const d = empty();
  run(d, 120, [
    { op: "task.create", id: id(1), title: "官网" },
    { op: "ai_job.create", id: id(2), kind: "tidy", text: "整理一下", local_time: "2026-11-07 10:00 星期六" },
  ]);
  expect(d.jobs.get(id(2))).toMatchObject({ kind: "tidy", state: "waiting", suggestions: null });
  // As the server has it by now: ready, with what AI suggests.
  const ready = { ...d.jobs.get(id(2))!, state: "ready" as const, suggestions: [{ task: id(1), title: "建好官网", after: [], reason: "以动作开头", was: { title: "官网", description: "", due: null } }] };
  d.jobs.set(id(2), ready);
  run(d, 121, [
    { op: "task.update", id: id(1), title: "建好官网" },
    { op: "ai_job.delete", id: id(2) },
  ]);
  expect(d.jobs.size).toBe(0);
  run(d, 122, [{ op: "undo", mutation: id(121) }]);
  expect(d.tasks.get(id(1))!.title).toBe("官网");
  expect(d.jobs.get(id(2))).toMatchObject({ state: "ready", suggestions: ready.suggestions });
  run(d, 123, [{ op: "ai_job.retry", id: id(2) }]);
  expect(d.jobs.get(id(2))).toMatchObject({ state: "waiting", suggestions: null });
});
