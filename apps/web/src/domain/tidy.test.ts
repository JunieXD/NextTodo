import { expect, test } from "vitest";
import type { AiJob } from "../api/types/AiJob";
import type { Suggestion } from "../api/types/Suggestion";
import type { Task } from "../api/types/Task";
import { apply, fromSnapshot, toSnapshot } from "./apply";
import { Board } from "./board";
import { groups, ops } from "./tidy";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const cx = { now: 1000, effectsOf: () => undefined };
const was = (title: string) => ({ title, description: "", due: null });

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

/** The tasks and dependencies (by number), a tidying of the root suggesting `suggestions`, and what
 * taking every part of them, or the parts `keep` keeps, leaves. */
function tidy(tasks: Task[], deps: [number, number][], suggestions: Suggestion[], keep: (key: string) => boolean = () => true) {
  const job: AiJob = { id: id(99), folder_id: null, kind: "tidy", text: "整理", local_time: "2026-11-09 10:00 星期一", state: "ready", error: null, suggestions, created_at: 0, updated_at: 0 };
  const dependencies = deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 }));
  const board = new Board({ folders: [], tasks, dependencies, ai_jobs: [job] }, "2026-11-12");
  const shown = groups(job, board);
  const taken = shown.flatMap((g) => g.parts.filter((p) => keep(p.key)));
  const d = fromSnapshot(board.snapshot);
  apply(d, ops(taken, job, board), cx);
  return { shown, after: new Board(toSnapshot(d), "2026-11-12") };
}

test("turned round, a dependency is taken away before the other way is added", () => {
  const { after } = tidy(
    [task(1, "订机票"), task(2, "订酒店")],
    [[1, 2]],
    [
      { task: id(2), after: [], unlink: [id(1)], reason: "", was: was("订酒店") },
      { task: id(1), after: [id(2)], reason: "", was: was("订机票") },
    ],
  );
  expect(after.snapshot.dependencies.map((d) => [d.before, d.after])).toEqual([[id(2), id(1)]]);
  expect(after.snapshot.ai_jobs).toEqual([]);
});

test("a new task left out takes its links with it", () => {
  const { shown, after } = tidy(
    [task(1, "搬家")],
    [],
    [
      { create: id(50), title: "打包行李", after: [], before: [id(1)], reason: "" },
      { task: id(1), title: "搬到新家", after: [id(50)], reason: "", was: was("搬家") },
    ],
    (key) => key !== "create",
  );
  expect(shown.find((g) => g.heading === "#1 搬家")!.parts.map((p) => p.text)).toEqual(["搬到新家", "“打包行李”"]);
  expect(after.task(id(50))).toBeUndefined();
  expect(after.task(id(1))!.title).toBe("搬到新家");
  expect(after.snapshot.dependencies).toEqual([]);
});

test("a new order puts the tasks named in the places they held, the others staying put", () => {
  const { after } = tidy([task(1, "看电影"), task(2, "买菜"), task(3, "洗车")], [], [{ order: [id(3), id(1)], reason: "" }]);
  expect(after.tasksIn(null).map((t) => t.number)).toEqual([3, 2, 1]);
});

test("done, a repeating task moves on; a deleted one goes with its links", () => {
  const due = { date: "2026-11-02", repeat: { every: 1, unit: "week" as const, from: "due" as const } };
  const { after } = tidy(
    [task(1, "交周报", { due }), task(2, "旧的"), task(3, "新的")],
    [[2, 3]],
    [
      { task: id(1), done: true, after: [], reason: "", was: { ...was("交周报"), due } },
      { delete: id(2), reason: "", was: was("旧的") },
    ],
  );
  expect(after.task(id(1))!.done_at).toBeNull();
  expect(after.task(id(1))!.due!.date > "2026-11-02").toBe(true);
  expect(after.task(id(2))).toBeUndefined();
  expect(after.snapshot.dependencies).toEqual([]);
});

test("what has changed since is shown as it is now, and what is done already is not shown", () => {
  const { shown } = tidy(
    [task(1, "官网（已改）"), task(2, "订机票", { done_at: 5 })],
    [],
    [
      { task: id(1), title: "建好官网", after: [], reason: "", was: was("官网") },
      { task: id(2), done: true, after: [], reason: "", was: was("订机票") },
    ],
    () => false,
  );
  expect(shown).toHaveLength(1);
  expect(shown[0].parts[0].now).toBe("整理后已改为：官网（已改）");
});
