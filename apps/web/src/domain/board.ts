// What follows from a user's data: task status, the order tasks are listed in, the steps along
// dependencies and folder paths.
// A twin of `crates/core/src/board.rs`; both are checked against `fixtures/board.json`.

import type { Attachment } from "../api/types/Attachment";
import type { Completion } from "../api/types/Completion";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Person } from "../api/types/Person";
import type { Share } from "../api/types/Share";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { isoDate, isoTime } from "./dates";
import { nextDue, shifted } from "./repeat";

export type Status = "ready" | "waiting" | "done";

/** Folder rank in the tree, manual order, id. */
type Key = [number, string, string];

const compare = (a: Key, b: Key) => a[0] - b[0] || cmp(a[1], b[1]) || cmp(a[2], b[2]);

/** Plain string order, as Rust compares strings. */
export const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Whether a task's title or description has `q`, given trimmed and in lower case. */
export const matches = (t: Task, q: string) => t.title.toLowerCase().includes(q) || t.description.toLowerCase().includes(q);

/** The user's day, `YYYY-MM-DD`, and time, `HH:MM`; without a time, all of the day has come. */
export interface Now {
  today: string;
  time?: string;
}

/** The moment `d` as the user's day and time. */
export const nowOf = (d: Date): Now => ({ today: isoDate(d), time: isoTime(d) });

/** Whether a start, its date with a time or without, is still to come `now`. */
export const before = (now: Now, date: string, time?: string) =>
  date > now.today || (date === now.today && time !== undefined && now.time !== undefined && time > now.time);

/** A day, and a time on it if any. */
type Moment = { date: string; time?: string };

/** When a repeating task done before holds up the tasks after it again: as its current occurrence
 * begins, at its start or else on its due date (docs/design.md#重复). */
function again(t: Task): Moment | null {
  if (!t.due?.repeat || t.done_at !== null || !t.completions) return null;
  return t.start !== null ? { date: t.start, time: t.start_time } : { date: t.due.date };
}

/** A snapshot indexed for lookups, as of a moment, the user's `now`, for the user `me`. `null` as a
 * folder means the root, the user's own. */
export class Board {
  private readonly tasks = new Map<TaskId, Task>();
  /** By owner and number: `${owner}#${number}`. */
  private readonly numbers = new Map<string, Task>();
  private readonly folders = new Map<FolderId, Folder>();
  private readonly people = new Map<string, Person>();
  /** The share each folder in a shared folder is in, the shared folder itself too. */
  private readonly shared = new Map<FolderId, Share>();
  /** The folders others share with the user, by who shares them and name. */
  private readonly foreign: Folder[] = [];
  private readonly prerequisites = new Map<TaskId, TaskId[]>();
  private readonly dependents = new Map<TaskId, TaskId[]>();
  /** Subfolders in manual order. */
  private readonly children = new Map<FolderId | null, Folder[]>();
  private readonly direct = new Map<FolderId | null, Task[]>();
  /** Position of each folder when the tree is listed top to bottom, root first. */
  private readonly rank = new Map<FolderId | null, number>();
  private readonly attached = new Map<TaskId, Attachment[]>();
  private readonly records = new Map<TaskId, Completion[]>();

  private steps?: Map<TaskId, [number, number]>;

  constructor(
    readonly snapshot: Snapshot,
    readonly now: Now,
    readonly me: string,
  ) {
    for (const p of snapshot.people) this.people.set(p.id, p);
    const shares = new Map(snapshot.shares.map((sh) => [sh.folder_id, sh]));
    for (const f of snapshot.folders) {
      this.folders.set(f.id, f);
      if (shares.get(f.id) && shares.get(f.id)!.owner !== me) this.foreign.push(f);
      else push(this.children, f.parent_id, f);
    }
    for (const list of this.children.values()) list.sort((a, b) => cmp(a.sort_key, b.sort_key) || cmp(a.id, b.id));
    const username = (f: Folder) => this.username(shares.get(f.id)!.owner);
    this.foreign.sort((a, b) => cmp(username(a), username(b)) || cmp(a.name, b.name) || cmp(a.id, b.id));
    const stack: [FolderId | null, Share | undefined][] = [...this.foreign].reverse().map((f) => [f.id, undefined]);
    stack.push([null, undefined]);
    while (stack.length) {
      const [f, outer] = stack.pop()!;
      this.rank.set(f, this.rank.size);
      const within = (f ? shares.get(f) : undefined) ?? outer;
      if (f && within) this.shared.set(f, within);
      stack.push(...this.subfolders(f).map((c): [FolderId, Share | undefined] => [c.id, within]).reverse());
    }
    for (const t of snapshot.tasks) {
      this.tasks.set(t.id, t);
      this.numbers.set(`${this.owner(t)}#${t.number}`, t);
      push(this.direct, t.folder_id, t);
    }
    for (const d of snapshot.dependencies) {
      push(this.prerequisites, d.after, d.before);
      push(this.dependents, d.before, d.after);
    }
    for (const a of snapshot.attachments) if (a.task_id) push(this.attached, a.task_id, a);
    // Ids are UUIDv7: in the order they were made.
    for (const list of this.attached.values()) list.sort((a, b) => cmp(a.id, b.id));
    for (const c of snapshot.completions) push(this.records, c.task_id, c);
    for (const list of this.records.values()) list.sort((a, b) => cmp(b.done, a.done) || cmp(b.due, a.due) || b.again - a.again);
  }

  task(id: TaskId): Task | undefined {
    return this.tasks.get(id);
  }

  attachments(t: TaskId): Attachment[] {
    return this.attached.get(t) ?? [];
  }

  /** The records of the times repeating task `t` was done, the latest first. */
  completions(t: TaskId): Completion[] {
    return this.records.get(t) ?? [];
  }

  /** The task with `number` among those of `owner`, a user id. */
  taskByNumber(owner: string, number: number): Task | undefined {
    return this.numbers.get(`${owner}#${number}`);
  }

  person(id: string): Person | undefined {
    return this.people.get(id);
  }

  /** Someone's display name; their id if this copy doesn't know them yet. */
  name(id: string): string {
    return this.people.get(id)?.display_name ?? id;
  }

  private username(id: string): string {
    return this.people.get(id)?.username ?? id;
  }

  /** The share a folder is in: its own, or that of the shared folder it is in. */
  share(f: FolderId | null): Share | undefined {
    return f ? this.shared.get(f) : undefined;
  }

  /** The shared folder `f` is, or is in. */
  sharedRoot(f: FolderId | null): FolderId | undefined {
    return this.share(f)?.folder_id;
  }

  /** Whose what is in folder `f` is: the owner of the shared folder it is in, or the user's. */
  ownerOf(f: FolderId | null): string {
    return this.share(f)?.owner ?? this.me;
  }

  /** Whose a task is. */
  owner(t: Task): string {
    return this.ownerOf(t.folder_id);
  }

  /** `#42`, or `@alice#42` for a task in a folder someone else shares. */
  label(t: Task): string {
    const owner = this.owner(t);
    return owner === this.me ? `#${t.number}` : `@${this.username(owner)}#${t.number}`;
  }

  /** Whether the user is to do it: in a shared folder, if it is assigned to them or to nobody. */
  mine(t: Task): boolean {
    return !this.share(t.folder_id) || !t.assignees.length || t.assignees.includes(this.me);
  }

  /** Whether `a` and `b` are in the same place, where tasks can be linked: both private, or in the
   * same shared folder (docs/design.md#共享). */
  together(a: Task, b: Task): boolean {
    return this.sharedRoot(a.folder_id) === this.sharedRoot(b.folder_id);
  }

  /** The folders others share with the user. */
  sharedWithMe(): Folder[] {
    return this.foreign;
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

  /** Whether its start is still to come. */
  notStarted(t: Task): boolean {
    return t.start !== null && before(this.now, t.start, t.start_time);
  }

  /** When the first of the user's open tasks still to start does, or a repeating task they wait for
   * holds them up again: the date, and the time if it has one. Shown with what is ready, so that
   * what shows it can be fetched again then. */
  nextStart(): Moment | null {
    const moments: Moment[] = [];
    for (const t of this.snapshot.tasks) {
      if (t.done_at !== null || !this.mine(t)) continue;
      if (t.start !== null) moments.push({ date: t.start, time: t.start_time });
      for (const id of this.prerequisites.get(t.id) ?? []) {
        const a = again(this.tasks.get(id)!);
        if (a) moments.push(a);
      }
    }
    let next: Moment | null = null;
    for (const m of moments) {
      if (!before(this.now, m.date, m.time)) continue;
      // A date alone starts the day: before any time on it.
      if (!next || cmp(m.date, next.date) < 0 || (m.date === next.date && cmp(m.time ?? "", next.time ?? "") < 0)) next = m;
    }
    return next;
  }

  /** How many prerequisites the task still waits for, now or at another moment `at`. */
  waitingOn(t: Task, at = this.now): number {
    return (this.prerequisites.get(t.id) ?? []).filter((id) => this.holdsUp(this.tasks.get(id)!, at)).length;
  }

  /** Whether the tasks after this one wait for it: it is not done, and if it repeats, it has not
   * been done yet or its current occurrence has begun (docs/design.md#重复). */
  holdsUp(t: Task, at = this.now): boolean {
    const a = again(t);
    return t.done_at === null && (!a || !before(at, a.date, a.time));
  }

  /** Whether a repeating task done now would still hold up the tasks after it: its next time has
   * begun by now too, as one caught up on is due today again. */
  private heldAgain(t: Task): boolean {
    const next = t.due?.repeat && nextDue(t.due.repeat, t.due.date, this.now.today);
    if (!t.due || !next) return false;
    return t.start !== null ? !before(this.now, shifted(t.start, t.due.date, next), t.start_time) : !before(this.now, next);
  }

  /** Tasks that become ready once this one is done. */
  unlocks(t: Task): Task[] {
    if (!this.holdsUp(t) || this.heldAgain(t)) return [];
    return this.after(t).filter((a) => a.done_at === null && this.waitingOn(a) === 1 && !this.notStarted(a));
  }

  /** Which step a task linked to others is along its dependencies, and how many there are on the
   * longest chain through it (docs/design.md#关联进度). Done tasks are steps too. */
  step(t: Task): [number, number] | undefined {
    if (!this.steps) {
      // Kahn's algorithm: each task after its prerequisites.
      const waiting = new Map([...this.prerequisites].map(([id, b]) => [id, b.length]));
      const order = [...this.dependents.keys()].filter((id) => !waiting.has(id));
      for (let i = 0; i < order.length; i++)
        for (const next of this.dependents.get(order[i]) ?? []) {
          const n = waiting.get(next)! - 1;
          waiting.set(next, n);
          if (n === 0) order.push(next);
        }
      const longest = (edges: Map<TaskId, TaskId[]>, counted: Map<TaskId, number>, id: TaskId) =>
        1 + Math.max(0, ...(edges.get(id) ?? []).map((other) => counted.get(other)!));
      const depth = new Map<TaskId, number>();
      for (const id of order) depth.set(id, longest(this.prerequisites, depth, id));
      const height = new Map<TaskId, number>();
      for (const id of [...order].reverse()) height.set(id, longest(this.dependents, height, id));
      this.steps = new Map(order.map((id) => [id, [depth.get(id)!, depth.get(id)! + height.get(id)! - 1]]));
    }
    return this.steps.get(t.id);
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

  /** Whether `f` is a shared folder or has one inside. */
  sharesWithin(f: FolderId): boolean {
    return this.snapshot.shares.some((sh) => this.inSubtree(sh.folder_id, f));
  }

  /** Whether folder `f` may move into `parent`: not into itself or a folder inside it, nor into a
   * shared folder when it is one or has one inside (docs/design.md#共享). */
  canMove(f: FolderId, parent: FolderId | null): boolean {
    return !this.inSubtree(parent, f) && !(this.sharedRoot(parent) && this.sharesWithin(f));
  }

  /** `/` for the root, otherwise `/工作/网站改版`; `@alice/装修/水电` in a folder Alice shares. */
  folderPath(f: FolderId | null): string {
    const names = this.names(f);
    const share = this.share(f);
    return share && share.owner !== this.me ? `@${this.username(share.owner)}/${names.join("/")}` : `/${names.join("/")}`;
  }

  private names(f: FolderId | null): string[] {
    const names = [];
    for (let cur = f; cur !== null; ) {
      const folder = this.folders.get(cur)!;
      names.unshift(folder.name);
      cur = folder.parent_id;
    }
    return names;
  }

  /** Where a folder is, as people read it: `学习 / 毕业论文`, `根目录`, or `Alice 的装修 / 水电` in a
   * folder Alice shares. */
  place(f: FolderId | null): string {
    if (!f) return this.folderName(null);
    const share = this.share(f);
    const place = this.names(f).join(" / ");
    return share && share.owner !== this.me ? `${this.name(share.owner)} 的${place}` : place;
  }

  /** A folder's name, or `根目录`. */
  folderName(f: FolderId | null): string {
    return f ? this.folders.get(f)!.name : "根目录";
  }

  /** The folder at a path such as `/工作/网站改版` (the leading `/` is optional) or `@alice/装修`. */
  findFolder(path: string): FolderId | null | undefined {
    let cur: FolderId | null = null;
    const names = path.split("/").map((s) => s.trim()).filter(Boolean);
    if (path.trimStart().startsWith("@")) {
      const [user, name] = [names.shift()!.slice(1).trim().toLowerCase(), names.shift()];
      const found = this.foreign.find((f) => f.name === name && this.username(this.shared.get(f.id)!.owner).toLowerCase() === user);
      if (!found) return undefined;
      cur = found.id;
    }
    for (const name of names) {
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

  /** Tasks in a folder and in its subfolders, in no particular order. */
  within(f: FolderId | null): Task[] {
    return [...this.contents(f), ...this.subfolders(f).flatMap((sub) => this.within(sub.id))];
  }

  /** Tasks directly in a folder: unfinished ones in dependency order, then finished ones, most
   * recently finished first. */
  tasksIn(f: FolderId | null): Task[] {
    const tasks = this.contents(f);
    const done = tasks.filter((t) => t.done_at !== null).sort((a, b) => b.done_at! - a.done_at! || cmp(a.id, b.id));
    return [...this.order(tasks.filter((t) => t.done_at === null), (t) => t.done_at === null), ...done];
  }

  /** Ready tasks of the user's, in the order of `byDue`. */
  next(): Task[] {
    return this.byDue([...this.tasks.values()].filter((t) => this.mine(t) && this.status(t) === "ready"));
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
