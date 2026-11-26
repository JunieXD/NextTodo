import { expect, test } from "vitest";
import type { AiJob } from "../api/types/AiJob";
import type { Folder } from "../api/types/Folder";
import type { Suggestion } from "../api/types/Suggestion";
import type { Task } from "../api/types/Task";
import { apply, fromSnapshot, toSnapshot } from "./apply";
import { Board } from "./board";
import { groups, ops } from "./tidy";

/** The user whose data it is. */
const ME = "019a4b2c-0000-7000-8000-0000000000ee";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const cx = { me: ME, now: 1000, local: { today: "2026-11-20" }, effectsOf: () => undefined, effectsAfter: () => [] };
const was = (title: string) => ({ title, description: "", due: null });

function task(n: number, title: string, extra: Partial<Task> = {}): Task {
  return {
    id: id(n),
    number: n,
    folder_id: null,
    title,
    description: "",
    done_at: null,
    dropped: false,
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

/** The tasks and dependencies (by number), a tidying of the root or folder `at` suggesting
 * `suggestions`, and what taking every part of them, or the parts `keep` keeps, leaves. */
function tidy(
  tasks: Task[],
  deps: [number, number][],
  suggestions: Suggestion[],
  keep: (key: string) => boolean = () => true,
  folders: Folder[] = [],
  at: number | null = null,
) {
  const job: AiJob = { id: id(99), folder_id: at === null ? null : id(at), kind: "tidy", text: "整理", local_time: "2026-11-09 10:00 星期一", state: "ready", error: null, suggestions, created_at: 0, updated_at: 0 };
  const dependencies = deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 }));
  const board = new Board({ folders, tasks, dependencies, completions: [], attachments: [], ai_jobs: [job], shares: [], people: [] }, { today: "2026-11-12" }, ME);
  const shown = groups(job, board);
  const taken = shown.flatMap((g) => g.parts.filter((p) => keep(p.key)));
  const d = fromSnapshot(board.snapshot);
  apply(d, ops(taken, job, board), cx);
  return { shown, after: new Board(toSnapshot(d), { today: "2026-11-12" }, ME) };
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

test("a date for a task due some days after its prerequisites shows what it was, and is a change also when the same", () => {
  const { shown, after } = tidy(
    [task(1, "报销", { due_after: { days: 7 } }), task(2, "复诊", { due: { date: "2026-12-02" }, due_after: { days: 7 }, unlocked_on: "2026-11-25" })],
    [],
    [
      // Made 30 days, then 7 on another device since.
      { task: id(1), due: { date: "2026-12-15" }, after: [], reason: "", was: { ...was("报销"), due_after: { days: 30 } } },
      { task: id(2), due: { date: "2026-12-02" }, after: [], reason: "", was: { ...was("复诊"), due: { date: "2026-12-02" }, due_after: { days: 7 } } },
    ],
  );
  const [claim, visit] = shown.map((g) => g.parts[0]);
  expect(claim.text).toMatch(/^前置完成后 30 天内 → /);
  expect(claim.now).toBe("整理后已改为：前置完成后 7 天内");
  expect(visit.text).toMatch(/（前置完成后 7 天内） → /);
  expect(visit.now).toBeUndefined();
  expect([after.task(id(2))!.due, after.task(id(2))!.due_after]).toEqual([{ date: "2026-12-02" }, undefined]);
});

test("every statutory working day and every Monday to Friday are told apart", () => {
  const weekdays = { every: 1, unit: "week" as const, from: "due" as const, days: [1, 2, 3, 4, 5] };
  const legal = { ...weekdays, legal: true };
  const { shown, after } = tidy(
    [task(1, "写日报", { due: { date: "2026-11-13", repeat: weekdays } }), task(2, "打卡", { due: { date: "2026-11-13", repeat: legal } })],
    [],
    [
      { task: id(1), due: { date: "2026-11-13", repeat: legal }, after: [], reason: "", was: { ...was("写日报"), due: { date: "2026-11-13", repeat: weekdays } } },
      // Made every statutory working day on another device since.
      { task: id(2), due: { date: "2026-11-13", time: "09:00", repeat: weekdays }, after: [], reason: "", was: { ...was("打卡"), due: { date: "2026-11-13", repeat: weekdays } } },
    ],
  );
  const [report, clockIn] = shown.map((g) => g.parts[0]);
  expect(report.now).toBeUndefined();
  expect(after.task(id(1))!.due!.repeat).toEqual(legal);
  expect(clockIn.now).toMatch(/^整理后已改为：.*每个法定工作日$/);
});

test("a folder's colour and icon are taken or left each, and one changed since is told of", () => {
  const folder = (n: number, name: string, look: Pick<Folder, "color" | "icon">, parent: number | null = null): Folder => ({
    id: id(n),
    parent_id: parent === null ? null : id(parent),
    name,
    sort_key: "a0",
    created_at: 0,
    updated_at: 0,
    ...look,
  });
  const { shown, after } = tidy(
    [],
    [],
    [
      { style: id(60), color: "blue", icon: "book", reason: "", was: { color: null, icon: null } },
      // Made green on another device since, given the icon suggested, and moved out of the folder tidied.
      { style: id(61), color: null, icon: "code", reason: "", was: { color: "red", icon: null } },
      // Of the name of the folder tidied, which it is in: told apart by its path.
      { style: id(62), icon: "pen", reason: "", was: { color: null, icon: null } },
    ],
    (key) => key !== "icon",
    [folder(60, "英语", { color: null, icon: null }), folder(61, "编程", { color: "green", icon: "code" }), folder(62, "英语", { color: null, icon: null }, 60)],
    60,
  );
  expect(shown.map((g) => [g.heading, g.parts.map((p) => [p.label, p.now])])).toEqual([
    ["文件夹“英语”", [["颜色", undefined], ["图标", undefined]]],
    ["文件夹“编程”", [["颜色", "整理后改过颜色"]]],
    ["文件夹“英语 / 英语”", [["图标", undefined]]],
  ]);
  expect([60, 61].map((n) => [after.folder(id(n))!.color, after.folder(id(n))!.icon])).toEqual([["blue", null], [null, "code"]]);
});
