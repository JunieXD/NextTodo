// What the dependency drawings show: the folder's tasks, the tasks elsewhere they are linked to,
// and the dependencies between them with redundant ones left out.

import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";

export type Edge = [before: TaskId, after: TaskId];

/** Leaves out A → C when A → B → C is there too. Dependencies have no cycles. */
export function reduce(edges: Edge[]): Edge[] {
  const next = new Map<TaskId, TaskId[]>();
  for (const [a, b] of edges) {
    const list = next.get(a);
    if (list) list.push(b);
    else next.set(a, [b]);
  }
  // For each task, everything after it, and everything after the tasks right after it: an edge to
  // one of those is implied by a longer path.
  const memo = new Map<TaskId, { after: Set<TaskId>; implied: Set<TaskId> }>();
  const walk = (n: TaskId) => {
    let m = memo.get(n);
    if (!m) {
      const direct = next.get(n) ?? [];
      const implied = new Set<TaskId>();
      for (const d of direct) for (const x of walk(d).after) implied.add(x);
      m = { after: new Set([...implied, ...direct]), implied };
      memo.set(n, m);
    }
    return m;
  };
  return edges.filter(([a, b]) => !walk(a).implied.has(b));
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
export function scene(board: Board, folder: FolderId, { done, sub }: { done: boolean; sub: boolean }): Scene {
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
