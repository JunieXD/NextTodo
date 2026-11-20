import { describe, expect, test } from "vitest";
import cases from "../../../../fixtures/board.json";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import { Board } from "./board";
import { agenda } from "./calendar";

interface Case {
  name: string;
  today?: string;
  time?: string;
  folders: { id: string; parent?: string; name: string; key: string }[];
  tasks: { id: string; folder?: string; key: string; done_at?: number; start?: string; start_time?: string; due?: Task["due"]; completions?: Task["completions"]; assignees?: string[] }[];
  deps: [string, string][];
  /** People by username; the user is `me`. */
  shares?: { folder: string; owner: string; members: string[] }[];
  expect: {
    status?: Record<string, string>;
    unlocks?: Record<string, string[]>;
    order?: Record<string, string[]>;
    next?: string[];
    paths?: Record<string, string>;
    /** The tasks in a folder and its subfolders, by name. */
    within?: Record<string, string[]>;
    steps?: Record<string, [number, number] | null>;
    /** `2026-11-12` or `2026-11-12 20:00`. */
    next_start?: string;
    /** By day, a time a repeating task is due later marked `+`. */
    agenda?: { days: number; overdue: string[]; due: Record<string, string[]> };
  };
}

/** The short names in a fixture as entity ids, the same ones the Rust test makes, given in the same
 * order. */
function ids() {
  const names: string[] = [];
  return (name: string) => {
    let i = names.indexOf(name);
    if (i < 0) i = names.push(name) - 1;
    return `00000000-0000-7000-8000-${String(i + 1).padStart(12, "0")}`;
  };
}

function snapshot(c: Case, id: (name: string) => string): Snapshot {
  const people = (usernames: string[] = []) => usernames.map(id).sort();
  const folders = c.folders.map((f) => ({
    id: id(f.id),
    parent_id: f.parent ? id(f.parent) : null,
    name: f.name,
    sort_key: f.key,
    color: null,
    icon: null,
    created_at: 0,
    updated_at: 0,
  }));
  const tasks = c.tasks.map((t, i) => ({
    id: id(t.id),
    number: i + 1,
    folder_id: t.folder ? id(t.folder) : null,
    title: t.id,
    description: "",
    done_at: t.done_at ?? null,
    start: t.start ?? null,
    start_time: t.start_time,
    due: t.due ?? null,
    completions: t.completions,
    sort_key: t.key,
    assignees: people(t.assignees),
    created_at: 0,
    updated_at: 0,
    created_by: id("me"),
    updated_by: id("me"),
    created_via: null,
    updated_via: null,
  }));
  const dependencies = c.deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 }));
  const shares = (c.shares ?? []).map((sh) => ({ folder_id: id(sh.folder), owner: id(sh.owner), members: people(sh.members) }));
  const usernames = [...new Set(["me", ...(c.shares ?? []).flatMap((sh) => [sh.owner, ...sh.members])])].sort();
  return { folders, tasks, dependencies, completions: [], attachments: [], ai_jobs: [], shares, people: usernames.map((u) => ({ id: id(u), username: u, display_name: u })) };
}

describe.each(cases as unknown as Case[])("$name", (c) => {
  const id = ids();
  const board = new Board(snapshot(c, id), { today: c.today ?? "2026-11-12", time: c.time }, id("me"));
  const task = (n: string) => board.task(id(n))!;
  const folder = (n: string) => (n === "/" ? null : id(n));
  const titles = (list: Task[]) => list.map((t) => t.title);

  test("status", () => {
    for (const [n, want] of Object.entries(c.expect.status ?? {})) {
      const t = task(n);
      const s = board.status(t);
      const on = board.waitingOn(t);
      const waiting = [on > 0 && `waiting ${on}`, board.notStarted(t) && "not started"].filter(Boolean).join(", ");
      expect(s === "waiting" ? waiting : s, n).toBe(want);
    }
  });
  test("unlocks", () => {
    for (const [n, want] of Object.entries(c.expect.unlocks ?? {})) expect(titles(board.unlocks(task(n))), n).toEqual(want);
  });
  test("order", () => {
    for (const [f, want] of Object.entries(c.expect.order ?? {})) expect(titles(board.tasksIn(folder(f))), f).toEqual(want);
  });
  test("next", () => {
    expect(titles(board.next())).toEqual(c.expect.next ?? []);
    const start = board.nextStart();
    expect(start && [start.date, start.time].filter(Boolean).join(" "), "next start").toEqual(c.expect.next_start ?? null);
  });
  test("steps", () => {
    for (const [n, want] of Object.entries(c.expect.steps ?? {})) expect(board.step(task(n)) ?? null, n).toEqual(want);
  });
  test("paths", () => {
    for (const [f, want] of Object.entries(c.expect.paths ?? {})) {
      expect(board.folderPath(folder(f))).toBe(want);
      expect(board.findFolder(want)).toBe(folder(f));
    }
  });
  test("within", () => {
    for (const [f, want] of Object.entries(c.expect.within ?? {})) expect(titles(board.within(folder(f))).sort(), f).toEqual(want);
  });
  test.runIf(c.expect.agenda)("agenda", () => {
    const want = c.expect.agenda!;
    const got = agenda(board, want.days);
    expect(titles(got.overdue), "overdue").toEqual(want.overdue);
    expect(got.days.map(([date, list]) => [date, list.map((d) => d.task.title + (d.later ? "+" : ""))])).toEqual(Object.entries(want.due));
  });
});

test("a repeating prerequisite done today holds what follows up again as its next time begins", () => {
  const repeat = { every: 1, unit: "day" as const, from: "due" as const };
  const c: Case = {
    name: "",
    folders: [],
    tasks: [
      { id: "words", key: "a0", due: { date: "2026-11-13", repeat }, completions: { count: 1, last: "2026-11-12" } },
      { id: "read", key: "a1" },
    ],
    deps: [["words", "read"]],
    expect: {},
  };
  const id = ids();
  const board = new Board(snapshot(c, id), { today: "2026-11-12", time: "21:00" }, id("me"));
  const read = board.task(id("read"))!;
  // As a reminder of it tomorrow morning comes.
  expect([board.waitingOn(read), board.waitingOn(read, { today: "2026-11-13", time: "09:00" })]).toEqual([0, 1]);
});
