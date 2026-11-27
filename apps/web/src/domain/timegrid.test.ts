import { expect, test } from "vitest";
import type { Completion } from "../api/types/Completion";
import type { Task } from "../api/types/Task";
import { Board } from "./board";
import { type Block, firstLook, fit, opensAt, slotOf, slotTime, weekBlocks } from "./timegrid";

const ME = "019a4b2c-0000-7000-8000-0000000000ee";
const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const task = (n: number, title: string, extra: Partial<Task>): Task => ({
  id: id(n), number: n, folder_id: null, title, description: "", done_at: null, dropped: false, start: null, due: null, sort_key: `a${n}`,
  created_at: 0, updated_at: 0, assignees: [], created_by: ME, updated_by: ME, created_via: null, updated_via: null, ...extra,
});
const board = (tasks: Task[], deps: [number, number][] = [], completions: Completion[] = []) =>
  new Board(
    {
      folders: [],
      tasks,
      dependencies: deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 })),
      completions,
      attachments: [],
      ai_jobs: [],
      shares: [],
      people: [], prefs: {},
    },
    { today: "2026-11-25", time: "10:20" },
    ME,
  );
const WEEK = ["2026-11-23", "2026-11-24", "2026-11-25", "2026-11-26", "2026-11-27", "2026-11-28", "2026-11-29"];
const NOW = new Date(2026, 10, 25, 10, 20);
const shown = (week: Map<string, Block[]>) =>
  Object.fromEntries([...week].filter(([, list]) => list.length).map(([day, list]) => [day, list.map((b) => `${b.task.title} ${b.time ?? "全天"} ${b.look}`)]));

test("a week's blocks: what is due each day at its time, done, given up, to come again and done before", () => {
  const b = board(
    [
      task(1, "交报告", { due: { date: "2026-11-24", time: "18:00" } }),
      task(2, "牙医复诊", { due: { date: "2026-11-26", time: "14:15" } }),
      task(3, "改稿", { due: { date: "2026-11-26", time: "14:00" } }),
      task(4, "订票", { due: { date: "2026-11-27" } }),
      task(5, "买菜", { due: { date: "2026-11-23", time: "09:00" }, done_at: 1 }),
      task(6, "退货", { due: { date: "2026-11-28" }, done_at: 1, dropped: true }),
      task(7, "开周会", { due: { date: "2026-11-25", time: "10:00", repeat: { every: 1, unit: "day", from: "due", until: "2026-11-27" } }, completions: { count: 1, last: "2026-11-24" } }),
      // Done for good the last time: there once, as the task.
      task(8, "喝药", { due: { date: "2026-11-24", time: "08:00" }, done_at: 1, completions: { count: 2, last: "2026-11-24" } }),
      // Another week.
      task(9, "交房租", { due: { date: "2026-12-01" } }),
      task(10, "没日子", {}),
    ],
    [[1, 3]],
    [
      { task_id: id(7), due: "2026-11-24", done: "2026-11-24", again: 0 },
      { task_id: id(8), due: "2026-11-23", done: "2026-11-23", again: 0 },
      { task_id: id(8), due: "2026-11-24", done: "2026-11-24", again: 0 },
    ],
  );
  expect(shown(weekBlocks(b, WEEK, NOW))).toEqual({
    "2026-11-23": ["喝药 08:00 done", "买菜 09:00 done"],
    "2026-11-24": ["喝药 08:00 done", "开周会 10:00 done", "交报告 18:00 overdue"],
    "2026-11-25": ["开周会 10:00 overdue"],
    "2026-11-26": ["开周会 10:00 later", "改稿 14:00 waiting", "牙医复诊 14:15 ready"],
    "2026-11-27": ["订票 全天 ready", "开周会 10:00 later"],
    "2026-11-28": ["退货 全天 dropped"],
  });
});

test("a time done twice before it ended the task is there for the first; given up, every time done is", () => {
  const b = board(
    [
      task(1, "跑步", { due: { date: "2026-11-26", time: "07:00" }, done_at: 1, completions: { count: 3, last: "2026-11-26" } }),
      task(2, "背单词", { due: { date: "2026-11-26", time: "21:00" }, done_at: 1, dropped: true, completions: { count: 1, last: "2026-11-25" } }),
    ],
    [],
    [
      { task_id: id(1), due: "2026-11-26", done: "2026-11-25", again: 0 },
      { task_id: id(1), due: "2026-11-26", done: "2026-11-26", again: 1 },
      { task_id: id(2), due: "2026-11-26", done: "2026-11-25", again: 0 },
    ],
  );
  const day = weekBlocks(b, WEEK, NOW).get("2026-11-26")!;
  expect(day.map((x) => `${x.task.title} ${x.look} ${x.record ? x.record.again : "task"}`)).toEqual(["跑步 done task", "跑步 done 0", "背单词 dropped task", "背单词 done 0"]);
});

test("one of several in a place goes with the rest; with room for one, all go together", () => {
  const blocks = (n: number) => Array.from({ length: n }, (_, i) => ({ task: task(i, `${i}`, {}), look: "ready", time: "14:00" }) as Block);
  const count = ({ shown, merged }: ReturnType<typeof fit>) => [shown.length, merged.length];
  expect(count(fit(blocks(2), 2))).toEqual([2, 0]);
  expect(count(fit(blocks(5), 3))).toEqual([2, 3]);
  expect(count(fit(blocks(3), 1))).toEqual([0, 3]);
  expect(count(fit(blocks(1), 1))).toEqual([1, 0]);
  const looks = (...l: Block["look"][]) => l.map((look) => ({ ...blocks(1)[0], look }));
  expect(firstLook(looks("done", "later", "waiting"))).toBe("waiting");
  expect(firstLook(looks("dropped", "ready", "overdue"))).toBe("overdue");
});

test("half hours: a time is in the one it begins in", () => {
  expect([slotOf("00:00"), slotOf("14:15"), slotOf("14:30"), slotOf("23:59")]).toEqual([0, 28, 29, 47]);
  expect([slotTime(0), slotTime(27), slotTime(47)]).toEqual(["00:00", "13:30", "23:30"]);
});

test("the grid opens before now this week, before the first time of another, at 8 with none", () => {
  const timed = new Map([["2026-12-01", [{ task: task(1, "a", {}), look: "ready", time: "13:30" } as Block]], ["2026-12-02", []]]);
  expect(opensAt(new Map([["2026-11-25", []]]), NOW, "2026-11-25")).toBe(10 * 60 + 20 - 90);
  expect(opensAt(timed, NOW, "2026-11-25")).toBe(12 * 60 + 30);
  expect(opensAt(new Map([["2026-12-01", []]]), NOW, "2026-11-25")).toBe(8 * 60);
  expect(opensAt(new Map([["2026-11-25", []]]), new Date(2026, 10, 25, 0, 30), "2026-11-25")).toBe(0);
});
