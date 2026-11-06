import { describe, expect, test } from "vitest";
import type { Snapshot } from "../api/types/Snapshot";
import { Board } from "./board";
import { railLayout } from "./rail";

const id = (n: number) => `00000000-0000-7000-8000-${String(n + 1).padStart(12, "0")}`;

/** `n` tasks in the root, in this order, with dependencies between them by index. */
function board(n: number, deps: [number, number][]) {
  const snapshot: Snapshot = {
    folders: [],
    tasks: Array.from({ length: n }, (_, i) => ({
      id: id(i),
      number: i + 1,
      folder_id: null,
      title: `t${i}`,
      description: "",
      done_at: null,
      due: null,
      sort_key: `a${String(i).padStart(3, "0")}`,
      created_at: 0,
      updated_at: 0,
      created_via: null,
      updated_via: null,
    })),
    dependencies: deps.map(([b, a]) => ({ before: id(b), after: id(a), created_at: 0 })),
    ai_jobs: [],
  };
  const b = new Board(snapshot);
  return { board: b, list: b.tasksIn(null) };
}

const layout = (n: number, deps: [number, number][]) => {
  const { board: b, list } = board(n, deps);
  return railLayout(list, b, Infinity)!;
};
const lanes = (n: number, deps: [number, number][]) => layout(n, deps).lanes;

describe("rail", () => {
  test("a chain takes one lane", () => {
    expect(lanes(4, [[0, 1], [1, 2], [2, 3]])).toBe(1);
  });

  test("a task with several after it takes one lane for them", () => {
    expect(lanes(6, [[0, 1], [0, 2], [0, 3], [0, 4], [0, 5]])).toBe(2);
  });

  test("dependencies into one task merge", () => {
    expect(lanes(6, [[0, 5], [1, 5], [2, 5], [3, 5], [4, 5]])).toBe(2);
  });

  test("both at once", () => {
    // 0 opens four; they all lead to 5.
    expect(lanes(6, [[0, 1], [0, 2], [0, 3], [0, 4], [1, 5], [2, 5], [3, 5], [4, 5]])).toBe(3);
  });

  test("a branch does not cross another line when sharing lanes would make it", () => {
    // Sharing 0's lane, 0 → 2 would bend across 1 → 3.
    const rail = layout(4, [[0, 2], [0, 3], [1, 3]]);
    expect([rail.crossings, rail.lanes]).toEqual([0, 3]);
    expect(layout(6, [[0, 1], [0, 2], [0, 3], [0, 4], [1, 5], [2, 5], [3, 5], [4, 5]]).crossings).toBe(0);
  });

  test("a folder whose rail would take too many lanes has none, worked out quickly", () => {
    // 125 tasks each before each of 125 more: 15625 dependencies, none implied by others.
    const deps: [number, number][] = [];
    for (let a = 0; a < 125; a++) for (let b = 125; b < 250; b++) deps.push([a, b]);
    const { board: b, list } = board(250, deps);
    const started = performance.now();
    expect(railLayout(list, b, 5)).toBeNull();
    expect(performance.now() - started).toBeLessThan(1000);
    const chain = board(3, [[0, 1], [1, 2]]);
    expect(railLayout(chain.list, chain.board, 1)).not.toBeNull();
  });

  test("no line runs through a task it does not link", () => {
    // Random graphs, each dependency forward in the list.
    let seed = 7;
    const random = () => (seed = (seed * 48271) % 2147483647) / 2147483647;
    for (let round = 0; round < 200; round++) {
      const n = 3 + Math.floor(random() * 10);
      const deps: [number, number][] = [];
      for (let i = 0; i < n; i++) for (let j = i + 1; j < n; j++) if (random() < 0.3) deps.push([i, j]);
      const { board: b, list } = board(n, deps);
      const rail = railLayout(list, b, Infinity)!;
      const row = new Map(list.map((t, i) => [t.id, i]));
      for (const p of rail.paths) {
        const [from, to] = p.key.split(">");
        const [ra, rb] = [row.get(from)!, row.get(to)!];
        // The lane: where the line is just above its second task's row.
        const lane = Number(new RegExp(`L(\\d+) ${rb * 64}`).exec(p.d)![1]);
        expect(p.d.startsWith(`M${rail.x(list[ra])} ${ra * 64 + 32}`)).toBe(true);
        expect(p.d.endsWith(`L${rail.x(list[rb])} ${rb * 64 + 32}`)).toBe(true);
        for (let r = ra + 1; r < rb; r++) expect(rail.x(list[r]), `${p.key} through row ${r}`).not.toBe(lane);
      }
    }
  });
});
