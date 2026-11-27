import { describe, expect, test } from "vitest";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import { Board } from "./board";
import { moves, rekey } from "./reorder";

/** The user whose data it is. */
const ME = "019a4b2c-0000-7000-8000-0000000000ee";

const FOLDER = "00000000-0000-7000-8000-000000000f00";
const OTHER = "00000000-0000-7000-8000-000000000f01";

const DIGITS = "0123456789ABCDEFGHIJ";
const id = (i: number) => `00000000-0000-7000-8000-${String(i).padStart(12, "0")}`;

/** Tasks `t0`, `t1`… in FOLDER (or OTHER), with the given keys, done or not, and dependencies. */
function board(tasks: { key: string; done?: boolean; other?: boolean }[], deps: [number, number][]): Board {
  const snapshot: Snapshot = {
    folders: [FOLDER, OTHER].map((f, i) => ({ id: f, parent_id: null, name: `f${i}`, sort_key: `a${i}`, color: null, icon: null, created_at: 0, updated_at: 0 })),
    tasks: tasks.map((t, i) => ({
      id: id(i),
      number: i + 1,
      folder_id: t.other ? OTHER : FOLDER,
      title: `t${i}`,
      description: "",
      done_at: t.done ? 1 : null,
      dropped: false,
      start: null,
      due: null,
      sort_key: t.key,
      created_at: 0,
      updated_at: 0,
      assignees: [],
      created_by: ME,
      updated_by: ME,
      created_via: null,
      updated_via: null,
    })),
    dependencies: deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 })),
    completions: [],
    attachments: [], ai_jobs: [],
    shares: [],
    people: [], prefs: {},
  };
  return new Board(snapshot, { today: "2026-11-12" }, ME);
}

const list = (b: Board) => b.order(b.contents(FOLDER).filter((t) => t.done_at === null), (t) => t.done_at === null);
const titles = (tasks: Task[]) => tasks.map((t) => t.title);

/** Where `l[from]` may go, each place with the titles of the list then. */
const places = (b: Board, l: Task[], from: number) => moves(b, l, from).to.map((m) => [m.at, m.rows().map((i) => l[i].title).join(" ")]);

describe("moves", () => {
  test("a task goes between the runs at its level, its own going along", () => {
    // t0 → t1 → t2, t0 → t3; t4 free
    const b = board([{ key: "a0" }, { key: "a1" }, { key: "a2" }, { key: "a3" }, { key: "a4" }], [
      [0, 1],
      [1, 2],
      [0, 3],
    ]);
    const l = list(b);
    expect(titles(l)).toEqual(["t0", "t1", "t2", "t3", "t4"]);
    // Before t3, t1 is where it is already.
    expect(places(b, l, 1)).toEqual([
      [1, "t0 t1 t2 t3 t4"],
      [2, "t0 t1 t2 t3 t4"],
      [3, "t0 t3 t1 t2 t4"],
    ]);
    expect(moves(b, l, 1).follows?.title).toBe("t0");
    expect(places(b, l, 4)).toEqual([
      [0, "t4 t0 t1 t2 t3"],
      [4, "t0 t1 t2 t3 t4"],
    ]);
    expect(moves(b, l, 4).follows).toBeUndefined();
  });

  test("a move gives a new key to the task moved alone", () => {
    // t0 → t3, t1 → t4, t2 → t5, made in that order: listed t0 t3 t1 t4 t2 t5, keys rising otherwise.
    const b = board(["a0", "a1", "a2", "a3", "a4", "a5"].map((key) => ({ key })), [
      [0, 3],
      [1, 4],
      [2, 5],
    ]);
    const l = list(b);
    expect(titles(l)).toEqual(["t0", "t3", "t1", "t4", "t2", "t5"]);
    const last = moves(b, l, 0).to.at(-1)!;
    expect(last.rows().map((i) => l[i].title)).toEqual(["t1", "t4", "t2", "t5", "t0", "t3"]);
    expect(last.keys().map((k) => k.id)).toEqual([id(0)]);
  });

  test("tasks with the same key keep it, unless the one moved goes between them", () => {
    // No dependencies; ordered by key, then ID.
    const b = board(["a0", "a0", "a1", "a1", "a2", "a3"].map((key) => ({ key })), []);
    const l = list(b);
    expect(titles(l)).toEqual(["t0", "t1", "t2", "t3", "t4", "t5"]);
    const last = moves(b, l, 4).to.at(-1)!;
    expect(last.rows().map((i) => l[i].title)).toEqual(["t0", "t1", "t2", "t3", "t5", "t4"]);
    expect(last.keys().map((k) => k.id)).toEqual([id(4)]);
    const between = moves(b, l, 2).to.find((m) => m.at === 1)!;
    expect(between.rows().map((i) => l[i].title)).toEqual(["t0", "t2", "t1", "t3", "t4", "t5"]);
    expect(between.keys().map((k) => k.id)).toEqual([id(2), id(1)]);
  });

  test("a task waiting for several follows the last of them, and goes with it when that one moves", () => {
    // t0 → t2, t1 → t2; t3 free
    const b = board([{ key: "a0" }, { key: "a1" }, { key: "a2" }, { key: "a3" }], [
      [0, 2],
      [1, 2],
    ]);
    const l = list(b);
    expect(titles(l)).toEqual(["t0", "t1", "t2", "t3"]);
    expect(places(b, l, 0)).toEqual([
      [0, "t0 t1 t2 t3"],
      [2, "t1 t0 t2 t3"],
      [3, "t1 t3 t0 t2"],
    ]);
  });

  test("dependencies through unfinished tasks elsewhere count, through finished ones not", () => {
    // t0 → t2 (elsewhere) → t1
    const tasks: { key: string; other?: boolean; done?: boolean }[] = [{ key: "a0" }, { key: "a1" }, { key: "a2", other: true }];
    const deps: [number, number][] = [
      [0, 2],
      [2, 1],
    ];
    const b = board(tasks, deps);
    expect(moves(b, list(b), 1).follows?.title).toBe("t0");
    tasks[2].done = true;
    const done = board(tasks, deps);
    expect(moves(done, list(done), 1).follows).toBeUndefined();
    expect(places(done, list(done), 1)).toEqual([
      [0, "t1 t0"],
      [1, "t0 t1"],
    ]);
  });

  test("finished tasks kept in the list hold others back as in its order", () => {
    // t0 (done) → t1, both listed, as in a folder keeping the latest done; t2 free
    const b = board([{ key: "a0", done: true }, { key: "a1" }, { key: "a2" }], [[0, 1]]);
    const l = b.order(b.contents(FOLDER), (t) => t.done_at === null);
    expect(titles(l)).toEqual(["t0", "t1", "t2"]);
    expect(places(b, l, 1)).toEqual([[1, "t0 t1 t2"]]);
    expect(moves(b, l, 1).follows?.title).toBe("t0");
    expect(places(b, l, 2)).toEqual([
      [0, "t2 t0 t1"],
      [2, "t0 t1 t2"],
    ]);
  });

  test("a task lands where shown, ahead of what was at its place, the rest keeping their order", () => {
    let seed = 42;
    const random = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
    for (let round = 0; round < 300; round++) {
      const n = 2 + Math.floor(random() * 7);
      // Random keys, so that the dependency order and the key order often disagree; some finished tasks
      // kept in the list, as in a folder keeping the latest done.
      const tasks = Array.from({ length: n }, () => ({ key: `a${DIGITS[Math.floor(random() * DIGITS.length)]}`, other: random() < 0.15, done: random() < 0.15, kept: random() < 0.5 }));
      const deps: [number, number][] = [];
      for (let a = 0; a < n; a++) for (let c = a + 1; c < n; c++) if (random() < 0.25) deps.push([a, c]);
      const listed = (b: Board) => b.order(b.contents(FOLDER).filter((t) => t.done_at === null || tasks[t.number - 1].kept), (t) => t.done_at === null);
      const b = board(tasks, deps);
      const l = listed(b);
      if (!l.length) continue;
      const from = Math.floor(random() * l.length);
      const t = l[from];
      const following = (x: Task, seen = new Set<string>()): Set<string> => {
        for (const a of b.after(x)) if ((a.done_at === null || l.includes(a)) && !seen.has(a.title)) following(a, seen.add(a.title));
        return seen;
      };
      const mine = following(t);
      const { to, follows } = moves(b, l, from);
      const level = l.filter((x, i) => x === t || (i !== from && moves(b, l, i).follows === follows));
      for (const m of to) {
        const key = new Map(m.keys().map((k) => [k.id, k.sort_key]));
        const moved = titles(listed(board(
          tasks.map((x, i) => ({ ...x, key: key.get(id(i)) ?? x.key })),
          deps,
        )));
        expect(moved).toEqual(m.rows().map((i) => l[i].title));
        // At its level, it goes before the task that was at its place, or last.
        const rest = l.filter((x) => x !== t);
        const next = rest.slice(m.at).find((x) => level.includes(x));
        const wanted = titles(level.filter((x) => x !== t));
        wanted.splice(next ? wanted.indexOf(next.title) : wanted.length, 0, t.title);
        expect(moved.filter((x) => wanted.includes(x))).toEqual(wanted);
        // Those not following it keep their order.
        const apart = titles(rest).filter((x) => !mine.has(x));
        expect(moved.filter((x) => apart.includes(x))).toEqual(apart);
      }
    }
  });
});

describe("rekey", () => {
  test("only the moved task gets a new key when the others are already in key order", () => {
    const b = board([{ key: "a0" }, { key: "a1" }, { key: "a2" }], []);
    const [t0, t1, t2] = list(b);
    expect(rekey([t2, t0, t1], t2.id).map((k) => k.id)).toEqual([t2.id]);
  });
});
