// The dependency rail on the left of a folder's list (docs/ui.md#轨道).

import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";
import { type Edge, edgesAmong } from "./graph";

const ROW = 64;
const LANE = 24;
const laneX = (c: number) => 28 + c * LANE;

/** Lanes like a subway map, each dependency running down one from its first task's row to the
 * second's. Dependencies may share a lane: those of a task with several after it, branching off to
 * each, and those into a task from tasks with one after them, merging. Of sharing both ways, one, or
 * neither, the layout with the fewest crossings is taken, then the one with the fewest lanes. */
export function railLayout(list: Task[], board: Board) {
  const index = new Map(list.map((t, i) => [t.id, i]));
  const edges = edgesAmong(board, list);
  const outs = new Map<TaskId, number>();
  for (const [a] of edges) outs.set(a, (outs.get(a) ?? 0) + 1);
  const fanOut = ([a]: Edge) => outs.get(a)! > 1;
  const best = [
    (e: Edge) => (fanOut(e) ? `from ${e[0]}` : `to ${e[1]}`),
    (e: Edge) => (fanOut(e) ? `from ${e[0]}` : e.join(">")),
    (e: Edge) => (fanOut(e) ? e.join(">") : `to ${e[1]}`),
    (e: Edge) => e.join(">"),
  ]
    .map((lineOf) => place(list, index, edges, lineOf))
    .reduce((x, y) => (y.crossings < x.crossings || (y.crossings === x.crossings && y.lanes < x.lanes) ? y : x));

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
  const { lanes, crossings, col } = best;
  return { lanes, crossings, gutter: 16 + lanes * LANE + 12, height: list.length * ROW, paths, x: (t: Task) => laneX(col.get(t.id)!) };
}

/** Puts each line (the dependencies `lineOf` gives one name) in a lane, row by row, and each task in
 * a lane that ends at it, or in a free one. A line from a task goes on straight down from it; one
 * that several tasks merge into runs beside them. */
function place(list: Task[], index: Map<TaskId, number>, edges: Edge[], lineOf: (e: Edge) => string) {
  const lines = new Map<string, { start: number; end: number; from: Set<TaskId> }>();
  for (const e of edges) {
    const l = lines.get(lineOf(e)) ?? { start: Infinity, end: -Infinity, from: new Set() };
    lines.set(lineOf(e), { start: Math.min(l.start, index.get(e[0])!), end: Math.max(l.end, index.get(e[1])!), from: l.from.add(e[0]) });
  }
  const merges = (k: string) => lines.get(k)!.from.size > 1;

  const lane = new Map<string, number>();
  const col = new Map<TaskId, number>();
  const taken: (string | null)[] = [];
  const free = (besides?: number) => {
    let i = 0;
    while (taken[i] || i === besides) i++;
    return i;
  };
  let lanes = 1;
  list.forEach((t, r) => {
    const ending = [...lines].filter(([, l]) => l.end === r).map(([k]) => lane.get(k)!);
    const c = ending.length ? Math.min(...ending) : free();
    col.set(t.id, c);
    for (const i of ending) taken[i] = null;
    // The one ending soonest goes on straight down.
    const starting = [...lines.keys()]
      .filter((k) => lines.get(k)!.start === r)
      .sort((x, y) => Number(merges(x)) - Number(merges(y)) || lines.get(x)!.end - lines.get(y)!.end);
    starting.forEach((k, n) => {
      const i = n === 0 && !merges(k) && !taken[c] ? c : free(c);
      taken[i] = k;
      lane.set(k, i);
    });
    lanes = Math.max(lanes, taken.length, c + 1);
  });

  // A bend between a task and a lane crossing another lane that runs there.
  const runs = edges.map((e) => {
    const [ra, rb, l] = [index.get(e[0])!, index.get(e[1])!, lane.get(lineOf(e))!];
    return { l, top: ra * ROW + (col.get(e[0]) === l ? 32 : 64), bottom: rb * ROW + (col.get(e[1]) === l ? 32 : 0) };
  });
  const crossed = new Set<string>();
  for (const e of edges) {
    const l = lane.get(lineOf(e))!;
    for (const [c, top] of [
      [col.get(e[0])!, index.get(e[0])! * ROW + 44],
      [col.get(e[1])!, index.get(e[1])! * ROW],
    ]) {
      for (const run of runs) {
        if (run.l > Math.min(c, l) && run.l < Math.max(c, l) && run.top < top + 20 && run.bottom > top) crossed.add(`${top} ${c} ${l} ${run.l}`);
      }
    }
  }
  return { lanes, crossings: crossed.size, col, lane: (e: Edge) => lane.get(lineOf(e))! };
}

/** The rail with no lines and the tasks in one lane, for a list whose rail would take too many. */
export const plainRail = { gutter: 16 + LANE + 12, x: laneX(0) };
