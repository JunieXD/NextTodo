import { expect, test } from "vitest";
import type { Effect } from "../api/types/Effect";
import type { Op } from "../api/types/Op";
import { type Data, Rejected, apply, fromSnapshot, revert } from "./apply";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const log = new Map<string, Effect[]>();
const cx = { now: 1000, effectsOf: (m: string) => log.get(m) };

function empty(): Data {
  return fromSnapshot({ folders: [], tasks: [], dependencies: [] });
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

test("revert undoes what replaying effects did", () => {
  const d = empty();
  const effects = run(d, 110, [
    { op: "task.create", id: id(1), title: "一" },
    { op: "task.update", id: id(1), title: "二" },
  ]);
  revert(d, effects);
  expect(d.tasks.size).toBe(0);
});
