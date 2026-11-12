import { describe, expect, test } from "vitest";
import cases from "../../../../fixtures/board.json";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import { Board } from "./board";

interface Case {
  name: string;
  today?: string;
  folders: { id: string; parent?: string; name: string; key: string }[];
  tasks: { id: string; folder?: string; key: string; done_at?: number; start?: string; due?: Task["due"] }[];
  deps: [string, string][];
  expect: {
    status?: Record<string, string>;
    unlocks?: Record<string, string[]>;
    order?: Record<string, string[]>;
    next?: string[];
    paths?: Record<string, string>;
    steps?: Record<string, [number, number] | null>;
  };
}

/** The short names in a fixture as entity ids, the same ones the Rust test makes. */
function ids() {
  const names: string[] = [];
  return (name: string) => {
    let i = names.indexOf(name);
    if (i < 0) i = names.push(name) - 1;
    return `00000000-0000-7000-8000-${String(i + 1).padStart(12, "0")}`;
  };
}

function snapshot(c: Case, id: (name: string) => string): Snapshot {
  return {
    folders: c.folders.map((f) => ({
      id: id(f.id),
      parent_id: f.parent ? id(f.parent) : null,
      name: f.name,
      sort_key: f.key,
      created_at: 0,
      updated_at: 0,
    })),
    tasks: c.tasks.map((t, i) => ({
      id: id(t.id),
      number: i + 1,
      folder_id: t.folder ? id(t.folder) : null,
      title: t.id,
      description: "",
      done_at: t.done_at ?? null,
      start: t.start ?? null,
      due: t.due ?? null,
      sort_key: t.key,
      created_at: 0,
      updated_at: 0,
      created_via: null,
      updated_via: null,
    })),
    dependencies: c.deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 })),
    ai_jobs: [],
  };
}

describe.each(cases as unknown as Case[])("$name", (c) => {
  const id = ids();
  const board = new Board(snapshot(c, id), c.today ?? "2026-11-12");
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
});
