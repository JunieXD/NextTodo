// The dependency rail on the left of a folder's list (docs/ui.md#轨道).

import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";
import { type Edge, edgesAmong } from "./graph";

const ROW = 64;
const LANE = 24;
const laneX = (c: number) => 28 + c * LANE;

/** Lanes like a subway map, each dependency running down one from its first task's row to the
 * second's, in at most `max` lanes; none when it can't be done. Dependencies may share a lane: those
 * of a task with several after it, branching off to each, and those into a task from tasks with one
 * after them, merging. Of sharing both ways, one, or neither, the layout with the fewest crossings
 * is taken, then the one with the fewest lanes. */
export function railLayout(list: Task[], board: Board, max: number) {
  const index = new Map(list.map((t, i) => [t.id, i]));
  const edges = edgesAmong(board, list);
  const outs = new Map<TaskId, number>();
  for (const [a] of edges) outs.set(a, (outs.get(a) ?? 0) + 1);
  const fanOut = ([a]: Edge) => outs.get(a)! > 1;
  // Crossings are counted only for layouts that fit: few lanes keep that cheap.
  const fits = [
    (e: Edge) => (fanOut(e) ? `from ${e[0]}` : `to ${e[1]}`),
    (e: Edge) => (fanOut(e) ? `from ${e[0]}` : e.join(">")),
    (e: Edge) => (fanOut(e) ? e.join(">") : `to ${e[1]}`),
    (e: Edge) => e.join(">"),
  ]
    .map((lineOf) => place(list, index, edges, lineOf, max))
    .filter((p) => p !== null)
    .map((p) => ({ ...p, crossings: crossings(p, edges, index) }));
  if (!fits.length) return null;
  const best = fits.reduce((x, y) => (y.crossings < x.crossings || (y.crossings === x.crossings && y.lanes < x.lanes) ? y : x));

  const paths = edges
    .map(([a, b]) => {
      const [xu, xl, xv] = [laneX(best.col.get(a)!), laneX(best.lane([a, b])), laneX(best.col.get(b)!)];
      const [yu, yv] = [index.get(a)! * ROW + 32, index.get(b)! * ROW + 32];
      let d = `M${xu} ${yu}`;
      if (xl !== xu) d += ` L${xu} ${yu + 12} C${xu} ${yu + 24} ${xl} ${yu + 20} ${xl} ${yu + 32}`;
      d += ` L${xl} ${yv - 32}`;
      if (xl !== xv) d += ` C${xl} ${yv - 20} ${xv} ${yv - 24} ${xv} ${yv - 12}`;
      d += ` L${xv} ${yv}`;
      return { key: `${a}>${b}`, d, live: board.status(board.task(a)!) === "ready" };
    })
    // Over the others where they share a lane.
    .sort((p, q) => Number(p.live) - Number(q.live));
  const { lanes, crossings: crossed, col } = best;
  return { lanes, crossings: crossed, gutter: 16 + lanes * LANE + 12, height: list.length * ROW, paths, x: (t: Task) => laneX(col.get(t.id)!) };
}

type Placed = NonNullable<ReturnType<typeof place>>;

/** Puts each line (the dependencies `lineOf` gives one name) in a lane, row by row, and each task in
 * a lane that ends at it, or in a free one; none when that takes more than `max` lanes. A line from
 * a task goes on straight down from it; one that several tasks merge into runs beside them. */
function place(list: Task[], index: Map<TaskId, number>, edges: Edge[], lineOf: (e: Edge) => string, max: number) {
  const lines = new Map<string, { start: number; end: number; from: Set<TaskId> }>();
  for (const e of edges) {
    const l = lines.get(lineOf(e)) ?? { start: Infinity, end: -Infinity, from: new Set() };
    lines.set(lineOf(e), { start: Math.min(l.start, index.get(e[0])!), end: Math.max(l.end, index.get(e[1])!), from: l.from.add(e[0]) });
  }
  const merges = (k: string) => lines.get(k)!.from.size > 1;
  const at = (row: (l: { start: number; end: number }) => number) => {
    const by = list.map((): string[] => []);
    for (const [k, l] of lines) by[row(l)].push(k);
    return by;
  };
  const [starting, ending] = [at((l) => l.start), at((l) => l.end)];

  const lane = new Map<string, number>();
  const col = new Map<TaskId, number>();
  const taken: (string | null)[] = [];
  const free = (besides?: number) => {
    let i = 0;
    while (taken[i] || i === besides) i++;
    return i;
  };
  let lanes = 1;
  for (const [r, t] of list.entries()) {
    const ends = ending[r].map((k) => lane.get(k)!);
    const c = ends.length ? Math.min(...ends) : free();
    col.set(t.id, c);
    for (const i of ends) taken[i] = null;
    // The one ending soonest goes on straight down. Past the last lane allowed, the rest are not worth
    // placing: a task can have thousands after it.
    const ordered = starting[r].sort((x, y) => Number(merges(x)) - Number(merges(y)) || lines.get(x)!.end - lines.get(y)!.end);
    for (const [n, k] of ordered.entries()) {
      const i = n === 0 && !merges(k) && !taken[c] ? c : free(c);
      if (i >= max) return null;
      taken[i] = k;
      lane.set(k, i);
    }
    lanes = Math.max(lanes, taken.length, c + 1);
    if (lanes > max) return null;
  }
  return { lanes, col, lane: (e: Edge) => lane.get(lineOf(e))! };
}

/** How many times a bend between a task and its lane crosses another lane where that runs: below
 * the task for a line leaving it, above for one coming in. */
function crossings({ col, lane }: Placed, edges: Edge[], index: Map<TaskId, number>) {
  // Where each lane runs, as heights.
  const runs = new Map<number, [top: number, bottom: number][]>();
  for (const e of edges) {
    const l = lane(e);
    const top = index.get(e[0])! * ROW + (col.get(e[0]) === l ? 32 : 64);
    const bottom = index.get(e[1])! * ROW + (col.get(e[1]) === l ? 32 : 0);
    if (!runs.has(l)) runs.set(l, []);
    runs.get(l)!.push([top, bottom]);
  }
  // Joined where they meet, in order down the lane.
  const spans = new Map([...runs].map(([l, r]) => [l, join(r)]));
  const bends = new Set<string>();
  for (const e of edges) {
    const l = lane(e);
    if (col.get(e[0]) !== l) bends.add(`${index.get(e[0])! * ROW + 44} ${col.get(e[0])} ${l}`);
    if (col.get(e[1]) !== l) bends.add(`${index.get(e[1])! * ROW} ${col.get(e[1])} ${l}`);
  }
  // Going down, with each lane's first span not yet passed.
  const next = new Map<number, number>();
  let n = 0;
  for (const [top, a, b] of [...bends].map((k) => k.split(" ").map(Number)).sort((x, y) => x[0] - y[0])) {
    for (let c = Math.min(a, b) + 1; c < Math.max(a, b); c++) {
      const s = spans.get(c) ?? [];
      let i = next.get(c) ?? 0;
      while (i < s.length && s[i][1] <= top) i++;
      next.set(c, i);
      if (i < s.length && s[i][0] < top + 20) n++;
    }
  }
  return n;
}

/** The heights `runs` cover, as spans in order and apart. */
function join(runs: [top: number, bottom: number][]) {
  const spans: [number, number][] = [];
  for (const [t, u] of runs.sort((x, y) => x[0] - y[0])) {
    const last = spans.at(-1);
    if (last && t <= last[1]) last[1] = Math.max(last[1], u);
    else spans.push([t, u]);
  }
  return spans;
}

/** The rail with no lines and the tasks in one lane, for a list whose rail would take too many. */
export const plainRail = { gutter: 16 + LANE + 12, x: laneX(0) };
