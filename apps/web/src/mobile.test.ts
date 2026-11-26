import { expect, test } from "vitest";
import type { Snapshot } from "./api/types/Snapshot";
import type { Task } from "./api/types/Task";
import { Board } from "./domain/board";
import { deviceReminders, widgetAgenda } from "./mobile";

const ME = "019a4b2c-0000-7000-8000-0000000000ee";
const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const task = (n: number, title: string, extra: Partial<Task>): Task => ({
  id: id(n), number: n, folder_id: null, title, description: "", done_at: null, dropped: false, start: null, due: null, sort_key: `a${n}`,
  created_at: 0, updated_at: 0, assignees: [], created_by: ME, updated_by: ME, created_via: null, updated_via: null, ...extra,
});

test("a reminder can't be done straight away once a repeating prerequisite holds it up again, also from a board behind the clock", () => {
  const snapshot: Snapshot = {
    folders: [],
    tasks: [
      task(1, "背单词", { due: { date: "2026-11-13", repeat: { every: 1, unit: "day", from: "due" } }, completions: { count: 1, last: "2026-11-12" } }),
      task(2, "做阅读", { due: { date: "2026-11-13", time: "00:00", reminders: [{ before: 0 }] } }),
    ],
    dependencies: [{ before: id(1), after: id(2), created_at: 0 }],
    completions: [], attachments: [], ai_jobs: [], shares: [], people: [],
  };
  // The board as of 23:59, handed over before midnight and again just after it.
  const board = new Board(snapshot, { today: "2026-11-12", time: "23:59" }, ME);
  const waiting = (at: string) => deviceReminders(board, false, new Date(at).getTime()).map((r) => r.waiting);
  expect([waiting("2026-11-12T23:59:30"), waiting("2026-11-13T00:00:02")]).toEqual([[1], [1]]);
});

test("the agenda widget lists what is due in the folders picked, with the later times of repeating ones to the end of next month's six weeks, and only days with some", () => {
  const folder = (n: number, name: string) => ({ id: id(n), parent_id: null, name, sort_key: `a${n}`, color: null, icon: null, created_at: 0, updated_at: 0 });
  const snapshot: Snapshot = {
    folders: [folder(10, "工作"), folder(11, "生活")],
    tasks: [
      task(1, "开例会", { folder_id: id(10), due: { date: "2026-11-19", time: "10:00", repeat: { every: 1, unit: "week", from: "due" } } }),
      task(2, "交报告", { folder_id: id(10), due: { date: "2026-11-19" } }),
      task(3, "买菜", { folder_id: id(11), due: { date: "2026-11-21" } }),
    ],
    dependencies: [], completions: [], attachments: [], ai_jobs: [], shares: [], people: [],
  };
  const { overdue, days } = widgetAgenda(new Board(snapshot, { today: "2026-11-20" }, ME), [id(10)]);
  expect(overdue.map((t) => [t.title, "later" in t])).toEqual([["开例会", false], ["交报告", false]]);
  expect(days[0].tasks.map((t) => [t.title, t.folder, t.due?.date, "later" in t])).toEqual([["开例会", "/工作", "2026-11-19", true]]);
  // December's six weeks end on Sunday 10 January.
  expect(days.map((d) => d.date)).toEqual(["2026-11-26", "2026-12-03", "2026-12-10", "2026-12-17", "2026-12-24", "2026-12-31", "2027-01-07"]);
});

test("the agenda widget says which tasks can be done now, and which time of a repeating one, to tick them off", () => {
  const snapshot: Snapshot = {
    folders: [],
    tasks: [
      task(1, "背单词", { due: { date: "2026-11-19", repeat: { every: 1, unit: "day", from: "due" } }, completions: { count: 2, last: "2026-11-18" } }),
      task(2, "写总结", { due: { date: "2026-11-19" } }),
      task(3, "收集数据", {}),
    ],
    dependencies: [{ before: id(3), after: id(2), created_at: 0 }],
    completions: [], attachments: [], ai_jobs: [], shares: [], people: [],
  };
  const { overdue } = widgetAgenda(new Board(snapshot, { today: "2026-11-20" }, ME), [null]);
  expect(overdue.map((t) => [t.title, t.status, t.completions?.count])).toEqual([["背单词", "ready", 2], ["写总结", "waiting", undefined]]);
});
