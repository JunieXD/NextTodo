// What follows from a user's data: task status, the order tasks are listed in and folder paths.
// A twin of `crates/core/src/board.rs`; both are checked against `fixtures/board.json`.

import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";

export type Status = "ready" | "waiting" | "done";

/** Folder rank in the tree, manual order, id. */
type Key = [number, string, string];

const compare = (a: Key, b: Key) => a[0] - b[0] || cmp(a[1], b[1]) || cmp(a[2], b[2]);

/** Plain string order, as Rust compares strings. */
export const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Whether a task's title or description has `q`, given trimmed and in lower case. */
export const matches = (t: Task, q: string) => t.title.toLowerCase().includes(q) || t.description.toLowerCase().includes(q);

/** A snapshot indexed for lookups, as of a day, the user's today (`YYYY-MM-DD`). `null` as a folder
 * means the root. */
export class Board {
  private readonly tasks = new Map<TaskId, Task>();
  private readonly numbers = new Map<number, Task>();
  private readonly folders = new Map<FolderId, Folder>();
  private readonly prerequisites = new Map<TaskId, TaskId[]>();
  private readonly dependents = new Map<TaskId, TaskId[]>();
  /** Subfolders in manual order. */
  private readonly children = new Map<FolderId | null, Folder[]>();
  private readonly direct = new Map<FolderId | null, Task[]>();
  /** Position of each folder when the tree is listed top to bottom, root first. */
  private readonly rank = new Map<FolderId | null, number>();

  constructor(
    readonly snapshot: Snapshot,
    readonly today: string,
  ) {
    for (const f of snapshot.folders) {
      this.folders.set(f.id, f);
      push(this.children, f.parent_id, f);
    }
    for (const list of this.children.values()) list.sort((a, b) => cmp(a.sort_key, b.sort_key) || cmp(a.id, b.id));
    const stack: (FolderId | null)[] = [null];
    while (stack.length) {
      const f = stack.pop()!;
      this.rank.set(f, this.rank.size);
      stack.push(...this.subfolders(f).map((c) => c.id).reverse());
    }
    for (const t of snapshot.tasks) {
      this.tasks.set(t.id, t);
      this.numbers.set(t.number, t);
      push(this.direct, t.folder_id, t);
    }
    for (const d of snapshot.dependencies) {
      push(this.prerequisites, d.after, d.before);
      push(this.dependents, d.before, d.after);
    }
  }

  task(id: TaskId): Task | undefined {
    return this.tasks.get(id);
  }

  taskByNumber(n: number): Task | undefined {
    return this.numbers.get(n);
  }

  folder(id: FolderId): Folder | undefined {
    return this.folders.get(id);
  }

  /** Prerequisites of a task. */
  before(t: Task): Task[] {
    return this.linked(this.prerequisites, t);
  }

  /** Tasks that wait for this one. */
  after(t: Task): Task[] {
    return this.linked(this.dependents, t);
  }

  private linked(edges: Map<TaskId, TaskId[]>, t: Task): Task[] {
    return this.sort((edges.get(t.id) ?? []).map((id) => this.tasks.get(id)!));
  }

  status(t: Task): Status {
    if (t.done_at !== null) return "done";
    return this.waitingOn(t) === 0 && !this.notStarted(t) ? "ready" : "waiting";
  }

  /** Whether its start date is still to come. */
  notStarted(t: Task): boolean {
    return t.start !== null && t.start > this.today;
  }

  /** How many prerequisites are not done yet. */
  waitingOn(t: Task): number {
    return (this.prerequisites.get(t.id) ?? []).filter((id) => this.tasks.get(id)!.done_at === null).length;
  }

  /** Tasks that become ready once this one is done. */
  unlocks(t: Task): Task[] {
    if (t.done_at !== null) return [];
    return this.after(t).filter((a) => a.done_at === null && this.waitingOn(a) === 1 && !this.notStarted(a));
  }

  /** The tasks on a shortest dependency chain from `from` to `to`, if there is one: linking
   * `to → from` would then close a loop. */
  chain(from: TaskId, to: TaskId): TaskId[] | undefined {
    const prev = new Map<TaskId, TaskId | null>([[from, null]]);
    const queue = [from];
    for (let i = 0; i < queue.length; i++) {
      const id = queue[i];
      if (id === to) {
        const path = [];
        for (let n: TaskId | null = to; n !== null; n = prev.get(n)!) path.unshift(n);
        return path;
      }
      for (const next of this.dependents.get(id) ?? []) {
        if (!prev.has(next)) {
          prev.set(next, id);
          queue.push(next);
        }
      }
    }
    return undefined;
  }

  subfolders(f: FolderId | null): Folder[] {
    return this.children.get(f) ?? [];
  }

  /** Whether there is a folder `f`: the root always, others until they are deleted. */
  hasFolder(f: FolderId | null): boolean {
    return f === null || this.folders.has(f);
  }

  /** The folder `f` is in. */
  parent(f: FolderId): FolderId | null {
    return this.folders.get(f)!.parent_id;
  }

  /** Every folder, root first, each followed by its subfolders. */
  folderTree(): (FolderId | null)[] {
    return [...this.rank.keys()].sort((a, b) => this.rank.get(a)! - this.rank.get(b)!);
  }

  /** How deep a folder is: 0 for the root. */
  depth(f: FolderId | null): number {
    let n = 0;
    for (let cur = f; cur !== null; cur = this.folders.get(cur)!.parent_id) n++;
    return n;
  }

  /** Whether `f` is `root` or inside it. */
  inSubtree(f: FolderId | null, root: FolderId | null): boolean {
    for (let cur = f; ; cur = this.folders.get(cur)!.parent_id) {
      if (cur === root) return true;
      if (cur === null) return false;
    }
  }

  /** `/` for the root, otherwise `/工作/网站改版`. */
  folderPath(f: FolderId | null): string {
    const names = [];
    for (let cur = f; cur !== null; ) {
      const folder = this.folders.get(cur)!;
      names.unshift(folder.name);
      cur = folder.parent_id;
    }
    return `/${names.join("/")}`;
  }

  /** Where a folder is, as people read it: `学习 / 毕业论文`, or `根目录`. */
  place(f: FolderId | null): string {
    return f ? this.folderPath(f).slice(1).replaceAll("/", " / ") : this.folderName(null);
  }

  /** A folder's name, or `根目录`. */
  folderName(f: FolderId | null): string {
    return f ? this.folders.get(f)!.name : "根目录";
  }

  /** The folder at a path such as `/工作/网站改版` (the leading `/` is optional). */
  findFolder(path: string): FolderId | null | undefined {
    let cur: FolderId | null = null;
    for (const name of path.split("/").map((s) => s.trim()).filter(Boolean)) {
      const found: Folder | undefined = this.subfolders(cur).find((f) => f.name === name);
      if (!found) return undefined;
      cur = found.id;
    }
    return cur;
  }

  /** Tasks directly in a folder, in no particular order. */
  contents(f: FolderId | null): Task[] {
    return this.direct.get(f) ?? [];
  }

  /** Tasks directly in a folder: unfinished ones in dependency order, then finished ones, most
   * recently finished first. */
  tasksIn(f: FolderId | null): Task[] {
    const tasks = this.contents(f);
    const done = tasks.filter((t) => t.done_at !== null).sort((a, b) => b.done_at! - a.done_at! || cmp(a.id, b.id));
    return [...this.order(tasks.filter((t) => t.done_at === null), (t) => t.done_at === null), ...done];
  }

  /** Ready tasks, in the order of `byDue`. */
  next(): Task[] {
    return this.byDue([...this.tasks.values()].filter((t) => this.status(t) === "ready"));
  }

  /** Tasks with a due date first, earliest first (a date without a time counts as the end of that
   * day), then the rest in folder order and manual order. */
  byDue(list: Task[]): Task[] {
    const due = (t: Task) => (t.due ? `${t.due.date} ${t.due.time ?? "24:00"}` : null);
    return list
      .map((t) => ({ t, due: due(t), key: this.key(t) }))
      .sort((a, b) => {
        if (a.due !== b.due) {
          if (a.due === null) return 1;
          if (b.due === null) return -1;
          return cmp(a.due, b.due);
        }
        return compare(a.key, b.key);
      })
      .map(({ t }) => t);
  }

  /** Tasks in folder order and manual order, ignoring dependencies. */
  sort(list: Task[]): Task[] {
    return list.sort((a, b) => compare(this.key(a), this.key(b)));
  }

  /** Orders tasks so that every task comes after the tasks it depends on, directly or through other
   * tasks for which `through` holds; otherwise in folder order and manual order. Among the tasks
   * that can go next, the one earliest in manual order goes first. */
  order(tasks: Task[], through: (t: Task) => boolean): Task[] {
    const members = new Set(tasks.map((t) => t.id));
    // The members and the tasks through which they depend on each other.
    const relevant = new Set<TaskId>();
    const stack = [...members];
    while (stack.length) {
      const id = stack.pop()!;
      if (relevant.has(id)) continue;
      relevant.add(id);
      for (const p of this.prerequisites.get(id) ?? []) {
        if (members.has(p) || through(this.tasks.get(p)!)) stack.push(p);
      }
    }
    // Kahn's algorithm. The other tasks go as soon as nothing holds them back; members wait in the
    // heap for their turn in manual order.
    const waiting = new Map<TaskId, number>();
    for (const id of relevant) waiting.set(id, (this.prerequisites.get(id) ?? []).filter((p) => relevant.has(p)).length);
    const free = [...waiting].filter(([, n]) => n === 0).map(([id]) => id);
    const heap = new Heap<Task>((a, b) => compare(this.key(a), this.key(b)));
    const list: Task[] = [];
    for (;;) {
      let id: TaskId;
      if (free.length) {
        id = free.pop()!;
        if (members.has(id)) {
          heap.push(this.tasks.get(id)!);
          continue;
        }
      } else if (heap.size) {
        const t = heap.pop();
        list.push(t);
        id = t.id;
      } else {
        break;
      }
      for (const next of this.dependents.get(id) ?? []) {
        const n = waiting.get(next);
        if (n === undefined) continue;
        waiting.set(next, n - 1);
        if (n === 1) free.push(next);
      }
    }
    return list;
  }

  private key(t: Task): Key {
    return [this.rank.get(t.folder_id)!, t.sort_key, t.id];
  }
}

function push<K, V>(map: Map<K, V[]>, key: K, value: V) {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

/** A binary min-heap. */
class Heap<T> {
  private items: T[] = [];

  constructor(private readonly compare: (a: T, b: T) => number) {}

  get size() {
    return this.items.length;
  }

  push(item: T) {
    const a = this.items;
    a.push(item);
    for (let i = a.length - 1; i > 0; ) {
      const parent = (i - 1) >> 1;
      if (this.compare(a[i], a[parent]) >= 0) break;
      [a[i], a[parent]] = [a[parent], a[i]];
      i = parent;
    }
  }

  pop(): T {
    const a = this.items;
    const top = a[0];
    const last = a.pop()!;
    if (a.length) {
      a[0] = last;
      for (let i = 0; ; ) {
        const l = 2 * i + 1;
        const r = l + 1;
        let m = i;
        if (l < a.length && this.compare(a[l], a[m]) < 0) m = l;
        if (r < a.length && this.compare(a[r], a[m]) < 0) m = r;
        if (m === i) break;
        [a[i], a[m]] = [a[m], a[i]];
        i = m;
      }
    }
    return top;
  }
}
