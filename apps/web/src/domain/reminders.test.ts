import { expect, test } from "vitest";
import type { Task } from "../api/types/Task";
import { Board } from "./board";
import { digestText, digests, reminders } from "./reminders";

/** The user whose data it is. */
const ME = "019a4b2c-0000-7000-8000-0000000000ee";

const FOLDER = "019a4b2c-0000-7000-8000-000000000f00";
const SHARED = "019a4b2c-0000-7000-8000-000000000f01";

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
    assignees: [],
    created_by: ME,
    updated_by: ME,
    created_via: null,
    updated_via: null,
    ...extra,
  };
}

test("reminders come at the times set, for the user's pending tasks, earliest first, past ones too, each moment of a task once", () => {
  const [alice, bob] = ["019a4b2c-0000-7000-8000-0000000000a1", "019a4b2c-0000-7000-8000-0000000000b0"];
  const board = new Board(
    {
      folders: [
        { id: FOLDER, parent_id: null, name: "学习", sort_key: "a0", color: null, icon: null, created_at: 0, updated_at: 0 },
        { id: SHARED, parent_id: null, name: "装修", sort_key: "a0", color: null, icon: null, created_at: 0, updated_at: 0 },
      ],
      tasks: [
        task(1, { due: { date: "2026-11-03", time: "18:00", reminders: [{ days: 1, time: "20:00" }, { before: 30 }] }, folder_id: FOLDER }),
        task(2, { due: { date: "2026-11-03", time: "12:00", reminders: [{ before: 0 }], repeat: { every: 1, unit: "week", from: "due" } }, completions: { count: 12, last: "2026-10-27" } }),
        task(3, { due: { date: "2026-11-03", time: "09:10", reminders: [{ before: 15 }] } }), // already past
        task(4, { due: { date: "2026-11-04", time: "08:00" } }), // no reminder
        task(5, { due: { date: "2026-11-05", time: "08:00", reminders: [{ before: 60 }] }, done_at: 1 }),
        // In a folder Alice shares: Bob's to do, and anyone's.
        task(6, { due: { date: "2026-11-05", time: "08:00", reminders: [{ before: 60 }] }, folder_id: SHARED, assignees: [bob] }),
        task(7, { due: { date: "2026-11-05", time: "09:00", reminders: [{ before: 0 }] }, folder_id: SHARED }),
        // On a date alone; and two at one moment.
        task(8, { due: { date: "2026-11-04", reminders: [{ days: 0, time: "09:00" }] } }),
        task(9, { due: { date: "2026-11-05", time: "09:00", reminders: [{ before: 0 }, { days: 0, time: "09:00" }] } }),
      ],
      dependencies: [],
      completions: [],
      attachments: [], ai_jobs: [],
      shares: [{ folder_id: SHARED, owner: alice, members: [bob, ME].sort() }],
      people: [
        { id: alice, username: "alice", display_name: "Alice" },
        { id: bob, username: "bob", display_name: "Bob" },
        { id: ME, username: "me", display_name: "我" },
      ],
    },
    { today: "2026-11-03" },
    ME,
  );
  expect(reminders(board, false).map((r) => [r.task.number, r.at.getDate(), r.at.getHours(), r.at.getMinutes(), r.body])).toEqual([
    [1, 2, 20, 0, "明天 18:00 截止 · 学习"],
    [3, 3, 8, 55, "今天 09:10 截止"],
    [2, 3, 12, 0, "今天 12:00 截止 · 已完成 12 次"],
    [1, 3, 17, 30, "今天 18:00 截止 · 学习"],
    [8, 4, 9, 0, "今天截止"],
    [7, 5, 9, 0, "今天 09:00 截止 · Alice 的装修"],
    [9, 5, 9, 0, "今天 09:00 截止"],
  ]);
  // A repeating task's are of its time now; a task that does not repeat has only the one.
  expect(reminders(board, false).flatMap((r) => (r.occurrence ? [[r.task.number, r.occurrence]] : []))).toEqual([[2, { due: "2026-11-03", completions: 12 }]]);
  // Hidden, they say only that something is due.
  expect(new Set(reminders(board, true).map((r) => `${r.title}|${r.body}`))).toEqual(new Set(["有事项到期|"]));
});

test("a daily digest each day from yesterday to a week on lists the open tasks due by then, earliest first, unless there are none", () => {
  const board = (tasks: Task[]) =>
    new Board({ folders: [], tasks, dependencies: [], completions: [], attachments: [], ai_jobs: [], shares: [], people: [] }, { today: "2026-11-03" }, ME);
  const tasks = [
    task(1, { title: "交房租", due: { date: "2026-11-03", time: "18:00" } }),
    task(2, { title: "还花呗", due: { date: "2026-11-01" } }), // overdue
    task(3, { title: "写周报", due: { date: "2026-11-05" } }),
    task(4, { title: "做完了", due: { date: "2026-11-02" }, done_at: 1 }),
    task(5, { title: "没截止" }),
  ];
  const said = digests(board(tasks), "23:30").map((d) => [d.at.getDate(), d.at.getHours(), d.at.getMinutes(), digestText(d.tasks.map((t) => t.title), false)]);
  const left = (titles: string[]) => ({ title: `今天还有 ${titles.length} 项没做完`, body: titles.join("、") });
  expect(said).toEqual([
    [2, 23, 30, left(["还花呗"])],
    [3, 23, 30, left(["还花呗", "交房租"])],
    [4, 23, 30, left(["还花呗", "交房租"])],
    ...[5, 6, 7, 8, 9].map((date) => [date, 23, 30, left(["还花呗", "交房租", "写周报"])]),
  ]);
  // Each task with its time, as its reminders have it.
  const daily = { every: 1, unit: "day", from: "due" } as const;
  expect(digests(board([...tasks, task(6, { due: { date: "2026-11-03", repeat: daily } })]), "23:30")[1].tasks.map((t) => [t.task, t.occurrence])).toEqual([
    [tasks[1].id, undefined],
    [tasks[0].id, undefined],
    [task(6, {}).id, { due: "2026-11-03", completions: 0 }],
  ]);
  // Hidden, only how many.
  expect(digestText(["还花呗", "交房租"], true)).toEqual({ title: "今天还有 2 项没做完", body: "" });
  // Nothing due until the 5th: no digest before.
  expect(digests(board([tasks[2], tasks[3], tasks[4]]), "21:00").map((d) => d.at.getDate())).toEqual([5, 6, 7, 8, 9]);
});
