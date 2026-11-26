// Moving tasks in a folder's list, ordered by dependencies (docs/design.md#排序), by hand or as AI
// tidying suggests, or a folder among its siblings: where a task may go, and the sort keys that put a
// list in the new order.

import { generateNKeysBetween } from "fractional-indexing";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";

export interface Move {
  /** Where it goes, as a place in the list without it. */
  at: number;
  /** The tasks then, as `Board.order` will list them: where each is now, first to last. */
  rows(): number[];
  /** The sort keys that put them so. */
  keys(): Keyed[];
}

export interface Moves {
  /** Where `list[from]` may go, first to last: its own place, before each task at its level, those
   * that follow what it follows, and after the last of them. */
  to: Move[];
  /** The task it follows: the last before it in the list that it waits for, directly or through tasks
   * in the list or unfinished ones. Finished itself, in a folder keeping the latest done, it holds
   * the others back all the same. */
  follows?: Task;
}

/** Where `list[from]` may go in `list`, a folder's tasks as it lists them. Keys only order the tasks
 * at each level among themselves: it alone gets a new one, as a rule. */
export function moves(board: Board, list: Task[], from: number): Moves {
  const up = parents(board, list);
  const parent = up[from];
  // Those at its level are in the run of the one it follows, which ends at the first task not after
  // that one; with nothing to follow, all the list.
  const under = parent < 0 ? undefined : reach(list[parent], (x) => board.after(x), new Set(list.map((x) => x.id)));
  let end = parent + 1;
  while (end < list.length && (under?.has(list[end].id) ?? true)) end++;
  const t = list[from];
  const others = list.filter((_, i) => i !== from && up[i] === parent);
  const index = new Map(list.map((x, i) => [x.id, i]));
  // Worked out only for the places it comes near, which with a long list are few of them.
  const plans = new Map<number, { rows: number[]; keys: Keyed[] }>();
  const plan = (k: number) => {
    if (!plans.has(k)) {
      const wanted = [...others];
      wanted.splice(k, 0, t);
      const keys = rekey(wanted, t.id);
      plans.set(k, { rows: relisted(board, list, keys).map((x) => index.get(x.id)!), keys });
    }
    return plans.get(k)!;
  };
  // Before the k-th of the others at its level, or after them all; as a place in the list without it.
  const move = (i: number, k: number): Move => ({ at: i > from ? i - 1 : i, rows: () => plan(k).rows, keys: () => plan(k).keys });
  const all = [move(from, others.filter((x) => index.get(x.id)! < from).length), ...others.map((x, k) => move(index.get(x.id)!, k)), move(end, others.length)];
  // Its own place and the one before the next at its level are the same.
  const to = all.filter((m, i) => all.findIndex((n) => n.at === m.at) === i).sort((x, y) => x.at - y.at);
  return { to, follows: parent < 0 ? undefined : list[parent] };
}

/** New sort keys for `list`, a folder's unfinished tasks as it lists them, that put the tasks of
 * `order` in that order as far as the dependencies let them, and the list then: they take the places
 * they held, and each task goes by the first of them it is or leads to (docs/ai.md#整理文件夹). None
 * when that brings them no closer to the order. */
export function arrange(board: Board, list: Task[], order: TaskId[]): { keys: Keyed[]; list: Task[] } {
  const queue = order.flatMap((id) => list.filter((t) => t.id === id));
  const wanted = list.map((t) => (order.includes(t.id) ? queue.shift()! : t));
  const place = new Map(wanted.map((t, i) => [t.id, i]));
  const listed = new Set(list.map((t) => t.id));
  const by = new Map(
    list.map((t) => {
      const led = [t.id, ...reach(t, (x) => board.after(x), listed)].filter((id) => order.includes(id) && place.has(id));
      return [t.id, led.length ? Math.min(...led.map((id) => place.get(id)!)) : place.get(t.id)!];
    }),
  );
  // Each level so, the tasks at it keeping their keys where they can.
  const up = parents(board, list);
  const keys = [...new Set(up)].flatMap((p) =>
    rekey(
      list.filter((_, i) => up[i] === p).sort((x, y) => by.get(x.id)! - by.get(y.id)! || place.get(x.id)! - place.get(y.id)!),
      "",
    ),
  );
  const then = relisted(board, list, keys);
  // How many pairs of them are in the order.
  const agree = (l: Task[]) => {
    const rank = l.flatMap((t) => (order.includes(t.id) ? [order.indexOf(t.id)] : []));
    return rank.reduce((n, r, i) => n + rank.slice(i + 1).filter((later) => later > r).length, 0);
  };
  return agree(then) > agree(list) ? { keys, list: then } : { keys: [], list };
}

/** For each task of `list`, a folder's tasks as it lists them, where the one it follows is: the last
 * before it that it waits for, directly or through tasks in the list or unfinished ones; -1 for none.
 * The tasks following the same one, or none, are a level, which `Board.order` lists in key order. */
function parents(board: Board, list: Task[]): number[] {
  const index = new Map(list.map((x, i) => [x.id, i]));
  return list.map((t) => {
    // On each way up the first in the list is the latest there: the rest come before it.
    let last = -1;
    const seen = new Set<TaskId>();
    const stack = [t];
    while (stack.length) {
      for (const p of board.before(stack.pop()!)) {
        if (seen.has(p.id)) continue;
        seen.add(p.id);
        const j = index.get(p.id);
        if (j !== undefined) last = Math.max(last, j);
        else if (p.done_at === null) stack.push(p);
      }
    }
    return last;
  });
}

/** The tasks `start` leads to by `next`, directly or through `listed` or unfinished tasks, as a folder
 * lists its tasks. */
function reach(start: Task, next: (t: Task) => Task[], listed: Set<TaskId>): Set<TaskId> {
  const found = new Set<TaskId>();
  const stack = [start];
  while (stack.length) {
    for (const n of next(stack.pop()!)) {
      if ((listed.has(n.id) || n.done_at === null) && !found.has(n.id)) {
        found.add(n.id);
        stack.push(n);
      }
    }
  }
  return found;
}

/** `list`, a folder's tasks, as it will list them with the sort keys `keys`. */
function relisted(board: Board, list: Task[], keys: Keyed[]): Task[] {
  const key = new Map(keys.map((k) => [k.id, k.sort_key]));
  const task = new Map(list.map((t) => [t.id, t]));
  return board
    .order(
      list.map((t) => (key.has(t.id) ? { ...t, sort_key: key.get(t.id)! } : t)),
      (t) => t.done_at === null,
    )
    .map((t) => task.get(t.id)!);
}

type Keyed = { id: string; sort_key: string };

/** New sort keys that keep `list` in this order: the tasks at a level of a list ordered by
 * dependencies (docs/design.md#排序), or anything else ordered by key, then ID.
 * The tasks already in order, as many as possible, keep their keys; those with the same key as the
 * one before a task put after it go after it too. */
export function rekey(list: Keyed[], moved: string): Keyed[] {
  const keep = new Set(increasing(list.filter((t) => t.id !== moved)).map((t) => t.id));
  const keys: Keyed[] = [];
  for (let i = 0; i < list.length; ) {
    if (keep.has(list[i].id)) {
      i++;
      continue;
    }
    let j = i;
    while (j < list.length && (!keep.has(list[j].id) || (i > 0 && list[j].sort_key === list[i - 1].sort_key))) j++;
    const between = generateNKeysBetween(i > 0 ? list[i - 1].sort_key : null, j < list.length ? list[j].sort_key : null, j - i);
    for (let k = i; k < j; k++) if (between[k - i] !== list[k].sort_key) keys.push({ id: list[k].id, sort_key: between[k - i] });
    i = j;
  }
  return keys;
}

/** A longest run of tasks, in list order, in order of key, then ID. */
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
      const last = list[tails[mid]];
      if (last.sort_key < t.sort_key || (last.sort_key === t.sort_key && last.id < t.id)) lo = mid + 1;
      else hi = mid;
    }
    prev[i] = lo > 0 ? tails[lo - 1] : -1;
    tails[lo] = i;
  });
  const run: Keyed[] = [];
  for (let i = tails.at(-1) ?? -1; i >= 0; i = prev[i]) run.push(list[i]);
  return run.reverse();
}
