import { expect, test } from "vitest";
import type { Task } from "../api/types/Task";
import { Board } from "./board";
import { reminders } from "./reminders";

const FOLDER = "019a4b2c-0000-7000-8000-000000000f00";

function task(n: number, extra: Partial<Task>): Task {
  return {
    id: `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`,
    number: n,
    folder_id: null,
    title: `t${n}`,
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

test("reminders come before the deadline by the time set, for pending synced tasks, earliest first", () => {
  const now = new Date(2026, 10, 3, 9, 0);
  const board = new Board({
    folders: [{ id: FOLDER, parent_id: null, name: "学习", sort_key: "a0", created_at: 0, updated_at: 0 }],
    tasks: [
      task(1, { due: { date: "2026-11-03", time: "18:00", remind_before: 30 }, folder_id: FOLDER }),
      task(2, { due: { date: "2026-11-03", time: "12:00", remind_before: 0 } }),
      task(3, { due: { date: "2026-11-03", time: "09:10", remind_before: 15 } }), // already past
      task(4, { due: { date: "2026-11-04", time: "08:00" } }), // no reminder
      task(5, { due: { date: "2026-11-05", time: "08:00", remind_before: 60 }, done_at: 1 }),
      task(0, { due: { date: "2026-11-05", time: "08:00", remind_before: 60 } }), // not synced yet
    ],
    dependencies: [],
    ai_jobs: [],
  }, "2026-11-03");
  expect(reminders(board, now).map((r) => [r.id, r.at.getHours(), r.at.getMinutes(), r.body])).toEqual([
    [2, 12, 0, "今天 12:00 截止"],
    [1, 17, 30, "今天 18:00 截止 · 学习"],
  ]);
});
