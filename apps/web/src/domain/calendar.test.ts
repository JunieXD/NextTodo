import { expect, test } from "vitest";
import type { Completion } from "../api/types/Completion";
import type { Task } from "../api/types/Task";
import { Board } from "./board";
import { doneByDay, dueBetween } from "./calendar";

const ME = "019a4b2c-0000-7000-8000-0000000000ee";
const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const task = (n: number, title: string, extra: Partial<Task>): Task => ({
  id: id(n), number: n, folder_id: null, title, description: "", done_at: null, start: null, due: null, sort_key: `a${n}`,
  created_at: 0, updated_at: 0, assignees: [], created_by: ME, updated_by: ME, created_via: null, updated_via: null, ...extra,
});
const board = (tasks: Task[], completions: Completion[] = []) =>
  new Board({ folders: [], tasks, dependencies: [], completions, attachments: [], ai_jobs: [], shares: [], people: [] }, { today: "2026-11-20" }, ME);

test("a repeating task is due each time on, as if done on the day, up to the day it repeats until", () => {
  const b = board([
    task(1, "练卷子", { due: { date: "2026-11-18", repeat: { every: 1, unit: "day", from: "due", until: "2026-11-21" } } }),
    task(2, "换滤芯", { due: { date: "2026-11-18", time: "09:00", repeat: { every: 10, unit: "day", from: "done" } } }),
    task(3, "交报告", { due: { date: "2026-11-19" }, done_at: 1 }),
  ]);
  const due = [...dueBetween(b, "2026-11-19", "2026-12-08")].map(([day, list]) => [day, list.map((d) => `${d.task.title}${d.later ? "（以后）" : ""}`)]);
  expect(due).toEqual([
    ["2026-11-19", ["练卷子（以后）"]],
    ["2026-11-20", ["练卷子（以后）"]],
    ["2026-11-21", ["练卷子（以后）"]],
    ["2026-11-28", ["换滤芯（以后）"]],
    ["2026-12-08", ["换滤芯（以后）"]],
  ]);
});

test("what was done on a day: tasks on the day done, and times repeating ones were done; the last of these as the task, on the day recorded", () => {
  const at = (day: number, hour: number) => new Date(2026, 10, day, hour).getTime();
  const b = board(
    [
      task(1, "交报告", { done_at: at(19, 21) }),
      task(2, "浇花", { due: { date: "2026-11-21", repeat: { every: 1, unit: "day", from: "due" } } }),
      // Done for good the last time, twice on the day.
      task(3, "练卷子", { due: { date: "2026-11-20" }, done_at: at(19, 21) }),
      // Done for good offline, the server taking it the day after.
      task(4, "背单词", { due: { date: "2026-11-18" }, done_at: at(20, 8) }),
    ],
    [
      { task_id: id(2), due: "2026-11-18", done: "2026-11-19" },
      { task_id: id(2), due: "2026-11-20", done: "2026-11-19" },
      { task_id: id(3), due: "2026-11-19", done: "2026-11-19" },
      { task_id: id(3), due: "2026-11-20", done: "2026-11-19" },
      { task_id: id(4), due: "2026-11-18", done: "2026-11-19" },
    ],
  );
  const done = doneByDay(b);
  const on = (day: string) => done.get(day)?.map((d) => [d.task.title, d.record?.due ?? null]);
  expect(on("2026-11-19")).toEqual([
    ["交报告", null],
    ["练卷子", null],
    ["背单词", null],
    ["浇花", "2026-11-18"],
    ["浇花", "2026-11-20"],
    ["练卷子", "2026-11-19"],
  ]);
  expect(on("2026-11-20")).toBeUndefined();
});
