import { expect, test } from "vitest";
import type { Snapshot } from "./api/types/Snapshot";
import type { Task } from "./api/types/Task";
import { Board } from "./domain/board";
import { deviceReminders } from "./mobile";

const ME = "019a4b2c-0000-7000-8000-0000000000ee";
const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const task = (n: number, title: string, extra: Partial<Task>): Task => ({
  id: id(n), number: n, folder_id: null, title, description: "", done_at: null, start: null, due: null, sort_key: `a${n}`,
  created_at: 0, updated_at: 0, assignees: [], created_by: ME, updated_by: ME, created_via: null, updated_via: null, ...extra,
});

test("a reminder can't be done straight away once a repeating prerequisite holds it up again, also from a board behind the clock", () => {
  const snapshot: Snapshot = {
    folders: [],
    tasks: [
      task(1, "背单词", { due: { date: "2026-11-13", repeat: { every: 1, unit: "day", from: "due" } }, completions: { count: 1, last: "2026-11-12" } }),
      task(2, "做阅读", { due: { date: "2026-11-13", time: "00:00", remind_before: 0 } }),
    ],
    dependencies: [{ before: id(1), after: id(2), created_at: 0 }],
    attachments: [], ai_jobs: [], shares: [], people: [],
  };
  // The board as of 23:59, handed over before midnight and again just after it.
  const board = new Board(snapshot, { today: "2026-11-12", time: "23:59" }, ME);
  const waiting = (at: string) => deviceReminders(board, new Date(at).getTime()).map((r) => r.waiting);
  expect([waiting("2026-11-12T23:59:30"), waiting("2026-11-13T00:00:02")]).toEqual([[1], [1]]);
});
