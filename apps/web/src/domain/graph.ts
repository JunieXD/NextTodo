// What the dependency drawings show: the folder's tasks, the tasks elsewhere they are linked to,
// and the dependencies between them with redundant ones left out.

import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";

export type Edge = [before: TaskId, after: TaskId];

/** Leaves out A → C when A → B → C is there too. Dependencies have no cycles. */
export function reduce(edges: Edge[]): Edge[] {
  const out = new Map<TaskId, Edge[]>();
  const waiting = new Map<TaskId, number>();
  for (const e of edges) {
    const list = out.get(e[0]);
    if (list) list.push(e);
    else out.set(e[0], [e]);
    waiting.set(e[0], waiting.get(e[0]) ?? 0);
    waiting.set(e[1], (waiting.get(e[1]) ?? 0) + 1);
  }
  // Each task's place in a topological order (Kahn's algorithm).
  const rank = new Map<TaskId, number>();
  const ready = [...waiting].filter(([, n]) => n === 0).map(([id]) => id);
  while (ready.length) {
    const id = ready.pop()!;
    rank.set(id, rank.size);
    for (const [, b] of out.get(id) ?? []) {
      waiting.set(b, waiting.get(b)! - 1);
      if (waiting.get(b) === 0) ready.push(b);
    }
  }
  // Going through a task's successors in that order, one that an earlier one leads to is implied.
  // Nothing past the last successor can be one, so the walks stop there.
  const redundant = new Set<Edge>();
  for (const list of out.values()) {
    if (list.length < 2) continue;
    const sorted = [...list].sort((x, y) => rank.get(x[1])! - rank.get(y[1])!);
    const last = rank.get(sorted.at(-1)![1])!;
    const reached = new Set<TaskId>();
    for (const e of sorted) {
      if (reached.has(e[1])) {
        redundant.add(e);
        continue;
      }
      const stack = [e[1]];
      while (stack.length) {
        for (const [, b] of out.get(stack.pop()!) ?? []) {
          if (rank.get(b)! <= last && !reached.has(b)) {
            reached.add(b);
            stack.push(b);
          }
        }
      }
    }
  }
  return edges.filter((e) => !redundant.has(e));
}

/** The dependencies among `tasks`, without redundant ones. */
export const edgesAmong = (board: Board, tasks: Task[]): Edge[] => {
  const ids = new Set(tasks.map((t) => t.id));
  return reduce(tasks.flatMap((t) => board.after(t).filter((a) => ids.has(a.id)).map((a): Edge => [t.id, a.id])));
};

export interface Scene {
  /** The folder's tasks in list order, then the linked tasks from elsewhere. */
  tasks: Task[];
  /** The tasks from elsewhere. */
  outside: Set<TaskId>;
  edges: Edge[];
}

/** The graph of a folder, with or without finished tasks and subfolders. */
export function scene(board: Board, folder: FolderId | null, { done, sub }: { done: boolean; sub: boolean }): Scene {
  const shown = (t: Task) => done || t.done_at === null;
  const folders = [folder];
  for (let i = 0; sub && i < folders.length; i++) folders.push(...board.subfolders(folders[i]).map((f) => f.id));
  const own = folders.flatMap((f) => board.tasksIn(f)).filter(shown);
  const ids = new Set(own.map((t) => t.id));
  const outside = new Map<TaskId, Task>();
  for (const t of own) {
    for (const o of [...board.before(t), ...board.after(t)]) if (!ids.has(o.id) && shown(o)) outside.set(o.id, o);
  }
  const tasks = [...own, ...outside.values()];
  return { tasks, outside: new Set(outside.keys()), edges: edgesAmong(board, tasks) };
}

/** Everything `id` waits for, directly or not, and everything waiting for it. */
export function lineage(edges: Edge[], id: TaskId): { up: Set<TaskId>; down: Set<TaskId> } {
  const walk = (from: 0 | 1) => {
    const next = new Map<TaskId, TaskId[]>();
    for (const e of edges) next.set(e[from], [...(next.get(e[from]) ?? []), e[1 - from]]);
    const found = new Set<TaskId>();
    const stack = [id];
    while (stack.length) {
      for (const m of next.get(stack.pop()!) ?? []) {
        if (!found.has(m)) {
          found.add(m);
          stack.push(m);
        }
      }
    }
    return found;
  };
  return { up: walk(1), down: walk(0) };
}
