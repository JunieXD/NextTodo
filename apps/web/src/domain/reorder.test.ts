import { describe, expect, test } from "vitest";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import { Board } from "./board";
import { range, rekey } from "./reorder";

const FOLDER = "00000000-0000-7000-8000-000000000f00";
const OTHER = "00000000-0000-7000-8000-000000000f01";

const DIGITS = "0123456789ABCDEFGHIJ";
const id = (i: number) => `00000000-0000-7000-8000-${String(i).padStart(12, "0")}`;

/** Tasks `t0`, `t1`… in FOLDER (or OTHER), with the given keys, done or not, and dependencies. */
function board(tasks: { key: string; done?: boolean; other?: boolean }[], deps: [number, number][]): Board {
  const snapshot: Snapshot = {
    folders: [FOLDER, OTHER].map((f, i) => ({ id: f, parent_id: null, name: `f${i}`, sort_key: `a${i}`, created_at: 0, updated_at: 0 })),
    tasks: tasks.map((t, i) => ({
      id: id(i),
      number: i + 1,
      folder_id: t.other ? OTHER : FOLDER,
      title: `t${i}`,
      description: "",
      done_at: t.done ? 1 : null,
      due: null,
      sort_key: t.key,
      created_at: 0,
      updated_at: 0,
      created_via: null,
      updated_via: null,
    })),
    dependencies: deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 })),
    ai_jobs: [],
  };
  return new Board(snapshot);
}

const list = (b: Board) => b.order(b.contents(FOLDER).filter((t) => t.done_at === null), (t) => t.done_at === null);
const titles = (tasks: Task[]) => tasks.map((t) => t.title);

describe("range", () => {
  test("a task stays between what it waits for and what waits for it", () => {
    // t0 → t1 → t2, t3 free
    const b = board([{ key: "a0" }, { key: "a1" }, { key: "a2" }, { key: "a3" }], [
      [0, 1],
      [1, 2],
    ]);
    const l = list(b);
    expect(titles(l)).toEqual(["t0", "t1", "t2", "t3"]);
    const r = range(b, l, 1);
    expect([r.min, r.max, r.above?.title, r.below?.title]).toEqual([1, 1, "t0", "t2"]);
    expect(range(b, l, 3)).toMatchObject({ min: 0, max: 3, above: undefined, below: undefined });
  });

  test("dependencies through unfinished tasks elsewhere count, through finished ones not", () => {
    // t0 → t2 (elsewhere) → t1
    const tasks: { key: string; other?: boolean; done?: boolean }[] = [{ key: "a0" }, { key: "a1" }, { key: "a2", other: true }];
    const deps: [number, number][] = [
      [0, 2],
      [2, 1],
    ];
    const b = board(tasks, deps);
    expect(range(b, list(b), 1)).toMatchObject({ min: 1, above: { title: "t0" } });
    tasks[2].done = true;
    const done = board(tasks, deps);
    expect(range(done, list(done), 1)).toMatchObject({ min: 0, above: undefined });
  });
});

describe("rekey", () => {
  test("only the moved task gets a new key when the others are already in key order", () => {
    const b = board([{ key: "a0" }, { key: "a1" }, { key: "a2" }], []);
    const [t0, t1, t2] = list(b);
    expect(rekey([t2, t0, t1], t2.id).map((k) => k.id)).toEqual([t2.id]);
  });

  test("any move within the range gives exactly the list asked for", () => {
    let seed = 42;
    const random = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
    for (let round = 0; round < 300; round++) {
      const n = 2 + Math.floor(random() * 7);
      // Random keys, so that the dependency order and the key order often disagree.
      const tasks = Array.from({ length: n }, () => ({ key: `a${DIGITS[Math.floor(random() * DIGITS.length)]}`, other: random() < 0.15, done: random() < 0.1 }));
      const deps: [number, number][] = [];
      for (let a = 0; a < n; a++) for (let c = a + 1; c < n; c++) if (random() < 0.25) deps.push([a, c]);
      const b = board(tasks, deps);
      const l = list(b);
      if (!l.length) continue;
      const from = Math.floor(random() * l.length);
      const { min, max } = range(b, l, from);
      const to = min + Math.floor(random() * (max - min + 1));
      const wanted = l.filter((_, i) => i !== from);
      wanted.splice(to, 0, l[from]);

      const keys = new Map(rekey(wanted, l[from].id).map((k) => [k.id, k.sort_key]));
      const moved = board(
        tasks.map((t, i) => ({ ...t, key: keys.get(id(i)) ?? t.key })),
        deps,
      );
      expect(titles(list(moved))).toEqual(titles(wanted));
    }
  });
});
