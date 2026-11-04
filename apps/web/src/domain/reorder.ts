// Moving a task by hand in a list ordered by dependencies, or a folder among its siblings: how far a
// task may go, and the sort keys that put a list in the new order.

import { generateNKeysBetween } from "fractional-indexing";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";

export interface Range {
  /** The first and last places `list[from]` may take in the list without it. */
  min: number;
  max: number;
  /** The closest task it waits for, directly or through unfinished tasks, and the closest one
   * waiting for it: what stops it going further. */
  above?: Task;
  below?: Task;
}

export function range(board: Board, list: Task[], from: number): Range {
  const t = list[from];
  const rest = list.filter((_, i) => i !== from);
  const linked = (next: (t: Task) => Task[]) => {
    const found = new Set<TaskId>();
    const stack = [t];
    while (stack.length) {
      for (const n of next(stack.pop()!)) {
        if (n.done_at === null && !found.has(n.id)) {
          found.add(n.id);
          stack.push(n);
        }
      }
    }
    return found;
  };
  const up = linked((x) => board.before(x));
  const down = linked((x) => board.after(x));
  const above = rest.findLastIndex((x) => up.has(x.id));
  const below = rest.findIndex((x) => down.has(x.id));
  return { min: above + 1, max: below < 0 ? rest.length : below, above: above < 0 ? undefined : rest[above], below: below < 0 ? undefined : rest[below] };
}

type Keyed = { id: string; sort_key: string };

/** New sort keys that keep `list` in this order. With keys increasing down a list that respects the
 * dependencies, the list orders itself this way, and so does anything else ordered by key alone.
 * The tasks whose keys already increase, as many as possible, keep them. */
export function rekey(list: Keyed[], moved: string): Keyed[] {
  const keep = new Set(increasing(list.filter((t) => t.id !== moved)).map((t) => t.id));
  const keys: Keyed[] = [];
  for (let i = 0; i < list.length; ) {
    if (keep.has(list[i].id)) {
      i++;
      continue;
    }
    let j = i;
    while (j < list.length && !keep.has(list[j].id)) j++;
    const between = generateNKeysBetween(i > 0 ? list[i - 1].sort_key : null, j < list.length ? list[j].sort_key : null, j - i);
    for (let k = i; k < j; k++) if (between[k - i] !== list[k].sort_key) keys.push({ id: list[k].id, sort_key: between[k - i] });
    i = j;
  }
  return keys;
}

/** A longest run of tasks, in list order, whose keys strictly increase. */
function increasing(list: Keyed[]): Keyed[] {
  // tails[n] ends the best run of length n + 1 found so far; prev links each task to the one before
  // it in its run.
  const tails: number[] = [];
  const prev: number[] = [];
  list.forEach((t, i) => {
    let lo = 0;
    let hi = tails.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (list[tails[mid]].sort_key < t.sort_key) lo = mid + 1;
      else hi = mid;
    }
    prev[i] = lo > 0 ? tails[lo - 1] : -1;
    tails[lo] = i;
  });
  const run: Keyed[] = [];
  for (let i = tails.at(-1) ?? -1; i >= 0; i = prev[i]) run.push(list[i]);
  return run.reverse();
}
