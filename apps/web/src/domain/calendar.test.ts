import { expect, test } from "vitest";
import type { Completion } from "../api/types/Completion";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import { apply, fromSnapshot, toSnapshot } from "./apply";
import { Board } from "./board";
import { completionDateOps, completionTimeOps, doneByDay, dueBetween } from "./calendar";

const ME = "019a4b2c-0000-7000-8000-0000000000ee";
const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const task = (n: number, title: string, extra: Partial<Task>): Task => ({
  id: id(n), number: n, folder_id: null, title, description: "", done_at: null, dropped: false, start: null, due: null, sort_key: `a${n}`,
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
      // Done for good the last time, which had been done the day before as well.
      task(5, "喝水", { due: { date: "2026-11-19" }, done_at: at(19, 21) }),
    ],
    [
      { task_id: id(2), due: "2026-11-18", done: "2026-11-19", again: 0 },
      { task_id: id(2), due: "2026-11-20", done: "2026-11-19", again: 0 },
      { task_id: id(3), due: "2026-11-19", done: "2026-11-19", again: 0 },
      { task_id: id(3), due: "2026-11-20", done: "2026-11-19", again: 0 },
      { task_id: id(4), due: "2026-11-18", done: "2026-11-19", again: 0 },
      { task_id: id(5), due: "2026-11-19", done: "2026-11-18", again: 0 },
      { task_id: id(5), due: "2026-11-19", done: "2026-11-19", again: 1 },
    ],
  );
  const done = doneByDay(b);
  const on = (day: string) => done.get(day)?.map((d) => [d.task.title, d.record?.due ?? null]);
  expect(on("2026-11-19")).toEqual([
    ["交报告", null],
    ["练卷子", null],
    ["背单词", null],
    ["喝水", null],
    ["浇花", "2026-11-18"],
    ["浇花", "2026-11-20"],
    ["练卷子", "2026-11-19"],
  ]);
  expect(on("2026-11-18")).toEqual([["喝水", "2026-11-19"]]);
  expect(on("2026-11-20")).toBeUndefined();
});

test("editing a completion so a repeat ends does not add a third completion to the calendar", () => {
  const t = task(1, "练卷子", {
    start: "2026-11-16",
    due: { date: "2026-11-17", repeat: { every: 2, unit: "day", from: "done", until: "2026-11-18" } },
    completions: { count: 2, last: "2026-11-15" },
  });
  const b = board([t], [
    { task_id: t.id, due: "2026-11-13", done: "2026-11-13", again: 0 },
    { task_id: t.id, due: "2026-11-15", done: "2026-11-15", again: 0 },
  ]);
  const d = fromSnapshot(b.snapshot);
  const now = new Date(2026, 10, 20, 9).getTime();
  apply(d, [{ op: "completion.update", task_id: t.id, due: "2026-11-15", again: 0, done: "2026-11-17" }], {
    me: ME, now, local: b.now, effectsOf: () => undefined, effectsAfter: () => [],
  });
  const ended = new Board(toSnapshot(d), b.now, ME);
  expect(ended.task(t.id)?.done_at).toBe(now);
  const done = doneByDay(ended);
  expect([...done].map(([day, list]) => [day, list.length]).sort()).toEqual([["2026-11-13", 1], ["2026-11-17", 1]]);
});

test("the last repeat's timestamp and completion record stay together from either editor", () => {
  const at = (day: number, hour: number, minute = 0) => new Date(2026, 10, day, hour, minute).getTime();
  const t = task(1, "练卷子", { due: { date: "2026-11-15" }, done_at: at(15, 17), completions: { count: 2, last: "2026-11-15" } });
  const previous = { task_id: t.id, due: "2026-11-14", done: "2026-11-14", again: 0 };
  const last = { task_id: t.id, due: "2026-11-15", done: "2026-11-15", again: 0 };
  const b = board([t], [previous, last]);
  const d = fromSnapshot(b.snapshot);
  const cx = { me: ME, now: at(20, 19), local: b.now, effectsOf: () => undefined, effectsAfter: () => [] };
  const edit = (ops: Op[], doneAt: number, dates: string[]) => {
    apply(d, ops, cx);
    const changed = new Board(toSnapshot(d), b.now, ME);
    const ended = changed.task(t.id)!;
    expect(ended.done_at).toBe(doneAt);
    expect(changed.snapshot.completions.map((c) => c.done).sort()).toEqual(dates);
    return { changed, ended };
  };
  const time = edit(completionTimeOps(b, t, at(16, 12, 30)), at(16, 12, 30), ["2026-11-14", "2026-11-16"]);
  const date = edit(completionDateOps(time.changed, time.ended, last, "2026-11-17"), at(17, 12, 30), ["2026-11-14", "2026-11-17"]);
  edit(completionDateOps(date.changed, date.ended, previous, "2026-11-13"), at(17, 12, 30), ["2026-11-13", "2026-11-17"]);
});
