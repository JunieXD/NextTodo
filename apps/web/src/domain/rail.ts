// The dependency rail on the left of a folder's list (docs/ui.md#轨道).

import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";
import { type Edge, edgesAmong } from "./graph";

const ROW = 64;
const LANE = 24;
const laneX = (c: number) => 28 + c * LANE;

/** Lanes like a subway map, each dependency running down one from its first task's row to the
 * second's. A task with several after it has one lane for them all, branching off to each; the
 * dependencies into a task from tasks with one after them merge into one, to the side of those
 * tasks, so that no branch crosses it. A task sits in a lane that ends at it, or in a free one. */
export function railLayout(list: Task[], board: Board) {
  const index = new Map(list.map((t, i) => [t.id, i]));
  const edges = edgesAmong(board, list);
  const outs = new Map<TaskId, number>();
  for (const [a] of edges) outs.set(a, (outs.get(a) ?? 0) + 1);
  const lineOf = ([a, b]: Edge) => (outs.get(a)! > 1 ? `from ${a}` : `to ${b}`);
  // The rows each line runs between, and whether several tasks merge into it.
  const lines = new Map<string, { start: number; end: number; merge: boolean }>();
  for (const e of edges) {
    const [s, t] = [index.get(e[0])!, index.get(e[1])!];
    const [k, l] = [lineOf(e), lines.get(lineOf(e))];
    lines.set(k, l ? { start: Math.min(l.start, s), end: Math.max(l.end, t), merge: k.startsWith("to") } : { start: s, end: t, merge: false });
  }

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
    // The task's own line ending soonest goes on straight down from it; one merging runs beside.
    const starting = [...lines].filter(([, l]) => l.start === r).sort(([, x], [, y]) => Number(x.merge) - Number(y.merge) || x.end - y.end);
    starting.forEach(([k, l], n) => {
      const i = n === 0 && !l.merge && !taken[c] ? c : free(c);
      taken[i] = k;
      lane.set(k, i);
    });
    lanes = Math.max(lanes, taken.length, c + 1);
  });

  const paths = edges
    .map(([a, b]) => {
      const [xu, xl, xv] = [laneX(col.get(a)!), laneX(lane.get(lineOf([a, b]))!), laneX(col.get(b)!)];
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
  return { lanes, gutter: 16 + lanes * LANE + 12, height: list.length * ROW, paths, x: (t: Task) => laneX(col.get(t.id)!) };
}

/** The rail with no lines and the tasks in one lane, for a list whose rail would take too many. */
export const plainRail = { gutter: 16 + LANE + 12, x: laneX(0) };
