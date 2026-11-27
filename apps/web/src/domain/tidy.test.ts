import { expect, onTestFinished, test, vi } from "vitest";
import type { AiJob } from "../api/types/AiJob";
import type { Folder } from "../api/types/Folder";
import type { Remind } from "../api/types/Remind";
import type { Suggestion } from "../api/types/Suggestion";
import type { Task } from "../api/types/Task";
import { apply, fromSnapshot, toSnapshot } from "./apply";
import { Board } from "./board";
import { groups, ops } from "./tidy";

/** The user whose data it is. */
const ME = "019a4b2c-0000-7000-8000-0000000000ee";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const askedAt = Date.UTC(2026, 10, 9, 2);
const cx = { me: ME, now: Date.UTC(2026, 10, 20), local: { today: "2026-11-20" }, effectsOf: () => undefined, effectsAfter: () => [] };
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
  const job: AiJob = { id: id(99), folder_id: at === null ? null : id(at), kind: "tidy", text: "整理", local_time: "2026-11-09 10:00 星期一", state: "ready", error: null, suggestions, created_at: askedAt, updated_at: askedAt };
  const dependencies = deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 }));
  const board = new Board({ folders, tasks, dependencies, completions: [], attachments: [], ai_jobs: [job], shares: [], people: [], prefs: {} }, { today: "2026-11-12" }, ME);
  const shown = groups(job, board, (_, p) => keep(p.key));
  const taken = shown.flatMap((g) => g.parts.filter((p) => keep(p.key)));
  const d = fromSnapshot(board.snapshot);
  const picked = ops(taken, job, board);
  apply(d, picked, cx);
  return { shown, board, picked, after: new Board(toSnapshot(d), { today: "2026-11-12" }, ME) };
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

test("a task put before one following another goes before that other, and the new order shown is the one it gets", () => {
  // 1 → 3: listed 1, 3, 2.
  const { shown, after } = tidy([task(1, "写初稿"), task(2, "买菜"), task(3, "改稿")], [[1, 3]], [{ order: [id(2), id(3)], reason: "" }]);
  expect(shown[0].parts[0].text).toBe("1. #2 买菜\n2. #3 改稿");
  expect(after.tasksIn(null).map((t) => t.number)).toEqual([2, 1, 3]);
});

test("an order the dependencies keep from having any effect is not shown", () => {
  // 2 → 3: listed 1, 2, 3; 3 can go no further up than after 2.
  const { shown } = tidy([task(1, "看电影"), task(2, "测试"), task(3, "上线")], [[2, 3]], [{ order: [id(3), id(2)], reason: "" }]);
  expect(shown).toEqual([]);
});

test("a new order goes by the dependencies as the suggestions taken with it leave them", () => {
  // 1 → 2, to be taken away, and 2 to go first.
  const tasks = [task(1, "买菜"), task(2, "洗车")];
  const suggestions: Suggestion[] = [
    { task: id(2), after: [], unlink: [id(1)], reason: "", was: was("洗车") },
    { order: [id(2), id(1)], reason: "" },
  ];
  const { shown, after } = tidy(tasks, [[1, 2]], suggestions);
  expect(shown.map((g) => g.key)).toEqual([id(2), "order"]);
  expect(after.tasksIn(null).map((t) => t.number)).toEqual([2, 1]);
  // Without the dependency taken away, the order can do nothing, and is not shown.
  const kept = tidy(tasks, [[1, 2]], suggestions, (key) => !key.startsWith("unlink"));
  expect(kept.shown.map((g) => g.key)).toEqual([id(2)]);
  expect(kept.after.tasksIn(null).map((t) => t.number)).toEqual([1, 2]);
  expect(kept.after.snapshot.dependencies).toHaveLength(1);
});

test("an order the dependency added with it would make anyway is shown only without it", () => {
  const tasks = [task(1, "买菜"), task(2, "洗车")];
  const suggestions: Suggestion[] = [
    { task: id(1), after: [id(2)], reason: "", was: was("买菜") },
    { order: [id(2), id(1)], reason: "" },
  ];
  expect(tidy(tasks, [], suggestions).shown.map((g) => g.key)).toEqual([id(1)]);
  const alone = tidy(tasks, [], suggestions, (key) => !key.startsWith("after"));
  expect(alone.shown.map((g) => g.key)).toEqual([id(1), "order"]);
  expect(alone.after.tasksIn(null).map((t) => t.number)).toEqual([2, 1]);
  expect(alone.after.snapshot.dependencies).toEqual([]);
});

test("an order brought no closer leaves the others where they are", () => {
  // 1 → 4 and 3 → 4: 4 cannot come before 3, and 2, not in the order, stays after 1.
  const { shown, after } = tidy([task(1, "A"), task(2, "U"), task(3, "X"), task(4, "B")], [[1, 4], [3, 4]], [{ order: [id(4), id(3)], reason: "" }]);
  expect(shown).toEqual([]);
  expect(after.tasksIn(null).map((t) => t.number)).toEqual([1, 2, 3, 4]);
});

test("a task put first goes first with what it waits for, as far as that lets it", () => {
  const { after } = tidy([task(1, "看电影"), task(2, "测试"), task(3, "上线")], [[2, 3]], [{ order: [id(3), id(1)], reason: "" }]);
  expect(after.tasksIn(null).map((t) => t.number)).toEqual([2, 3, 1]);
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

test("a due date and the reminders of all kinds are one part; those by a start it no longer has a time for are left out; only what it sets is checked for changes since", () => {
  vi.useFakeTimers({ toFake: ["Date"], now: new Date(2026, 10, 12, 10, 0) });
  onTestFinished(() => void vi.useRealTimers());
  const at = [{ date: "2026-12-15", time: "10:00" }];
  const { shown, after } = tidy(
    [
      task(1, "开会", { start: "2026-12-01", start_time: "09:00" }),
      // Its start's time taken off since: of those by it, those on a day still go.
      task(2, "抢票", { start: "2026-12-01" }),
      task(4, "订餐", { start: "2026-12-01" }),
      // Its reminders by the start changed since, which this one doesn't set.
      task(3, "交报告", { start: "2026-12-01", start_reminders: [{ before: 0 }] }),
    ],
    [],
    [
      { task: id(1), due: { date: "2026-12-20", reminders: [{ days: 0, time: "09:00" }] }, start_reminders: [{ before: 30 }], remind_at: at, after: [], reason: "", was: was("开会") },
      { task: id(2), start_reminders: [{ before: 30 }], after: [], reason: "", was: { ...was("抢票"), start_reminders: [] } },
      { task: id(3), remind_at: at, after: [], reason: "", was: was("交报告") },
      { task: id(4), start_reminders: [{ before: 30 }, { days: 0, time: "09:00" }], after: [], reason: "", was: was("订餐") },
    ],
  );
  expect(shown.map((g) => g.parts.map((p) => [p.label, p.text, p.now]))).toEqual([
    [["截止", "无 → 12月20日 · 当天 09:00、开始前 30 分钟、12月15日 10:00", undefined]],
    [["提醒", "开始时 → 开始时、12月15日 10:00", undefined]],
    [["提醒", "无 → 开始当天 09:00", undefined]],
  ]);
  // Asked to confirm by what it sets: what else changed since leaves that be.
  const confirming = (start_reminders: Remind[]) =>
    tidy(
      [task(3, "交报告", { start: "2026-12-01", start_reminders, remind_at: [{ date: "2026-12-10", time: "08:00" }] })],
      [],
      [{ task: id(3), remind_at: at, after: [], reason: "", was: was("交报告") }],
    ).shown[0].parts[0].current;
  expect(confirming([{ before: 0 }])).toBeDefined();
  expect(confirming([{ before: 0 }])).toBe(confirming([{ before: 5 }]));
  expect([after.task(id(1))!.due, after.task(id(1))!.start_reminders, after.task(id(1))!.remind_at]).toEqual([
    { date: "2026-12-20", reminders: [{ days: 0, time: "09:00" }] },
    [{ before: 30 }],
    at,
  ]);
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

test("taking AI completion dates shifts the job's timestamp, except for recurring occurrences", () => {
  const due = { date: "2026-11-06", repeat: { every: 1, unit: "day", from: "done" } } as const;
  const { after } = tidy(
    [task(1, "开组会"), task(2, "打卡", { due }), task(3, "交报告", { done_at: 900, dropped: true })],
    [],
    [
      { task: id(1), done: "2026-11-08", after: [], reason: "", was: was("开组会") },
      { task: id(2), done: "2026-11-08", after: [], reason: "", was: { ...was("打卡"), due, occurrence: { due: due.date, completions: 0 } } },
      { task: id(3), done: "2026-11-07", after: [], reason: "", was: { ...was("交报告"), done_at: 900 } },
      { create: id(4), title: "完成培训", done: "2026-11-09", after: [], before: [], reason: "" },
    ],
  );
  expect([1, 2, 3, 4].map((i) => after.task(id(i))!.done_at)).toEqual([Date.UTC(2026, 10, 8, 2), null, Date.UTC(2026, 10, 7, 2), askedAt]);
  expect(after.task(id(3))!.dropped).toBe(true);
  expect(after.completions(id(2)).map((c) => c.done)).toEqual(["2026-11-08"]);
  expect(after.task(id(2))!.due?.date).toBe("2026-11-09");
});

test("a suggestion to date an already completed task leaves it open if reopened since", () => {
  const { shown, after } = tidy([task(1, "开组会")], [], [
    { task: id(1), title: "开项目组会", done: "2026-11-08", after: [], reason: "", was: { ...was("开组会"), done_at: askedAt } },
  ]);
  expect(shown.flatMap((g) => g.parts.map((p) => p.key))).toEqual(["text"]);
  expect(after.task(id(1))).toMatchObject({ title: "开项目组会", done_at: null });
});

test("a suggestion queued before another device checked in does not complete the next occurrence", () => {
  const due = { date: "2026-11-08", repeat: { every: 1, unit: "day", from: "done" } } as const;
  const { board, picked, after } = tidy(
    [task(1, "打卡", { due })],
    [],
    [{ task: id(1), done: "2026-11-08", after: [], reason: "", was: { ...was("打卡"), due, occurrence: { due: due.date, completions: 0 } } }],
  );
  expect(after.completions(id(1)).map((c) => c.done)).toEqual(["2026-11-08"]);
  const d = fromSnapshot(board.snapshot);
  apply(d, [{ op: "task.update", id: id(1), done: true, today: "2026-11-07" }], cx);
  apply(d, picked, cx);
  const synced = new Board(toSnapshot(d), board.now, ME);
  expect(synced.task(id(1))).toMatchObject({ done_at: null, due, completions: { count: 1, last: "2026-11-07" } });
  expect(synced.completions(id(1)).map((c) => c.done)).toEqual(["2026-11-07"]);
});

test("taking both a new deadline and a completion date still finishes the same occurrence", () => {
  const repeat = { every: 1, unit: "day", from: "due" } as const;
  const due = { date: "2026-11-07", repeat };
  const { after } = tidy([task(1, "打卡", { due })], [], [
    { task: id(1), due: { date: "2026-11-09", repeat }, done: "2026-11-08", after: [], reason: "", was: { ...was("打卡"), due, occurrence: { due: due.date, completions: 0 } } },
  ]);
  expect(after.completions(id(1))).toEqual([{ task_id: id(1), due: "2026-11-09", done: "2026-11-08", again: 0 }]);
  expect(after.task(id(1))?.due?.date).toBe("2026-11-10");
});
