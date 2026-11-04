// Applies operations to a local copy the way the server does (`crates/server/src/mutate.rs`), and
// yields the same kind of effects, so the app shows a change at once and can undo it locally.
// What only the server knows stays unknown: a new task's number is 0 until the server's copy arrives.

import { generateKeyBetween } from "fractional-indexing";
import type { AiJob } from "../api/types/AiJob";
import type { AiJobId } from "../api/types/AiJobId";
import type { Dependency } from "../api/types/Dependency";
import type { Effect } from "../api/types/Effect";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { cmp } from "./board";

/** A user's data, keyed for changes. */
export interface Data {
  folders: Map<FolderId, Folder>;
  tasks: Map<TaskId, Task>;
  deps: Map<string, Dependency>;
  jobs: Map<AiJobId, AiJob>;
}

export const depKey = (before: TaskId, after: TaskId) => `${before}>${after}`;

export function fromSnapshot(s: Snapshot): Data {
  return {
    folders: new Map(s.folders.map((f) => [f.id, f])),
    tasks: new Map(s.tasks.map((t) => [t.id, t])),
    deps: new Map(s.dependencies.map((d) => [depKey(d.before, d.after), d])),
    // Copies saved before there were AI jobs have none.
    jobs: new Map((s.ai_jobs ?? []).map((j) => [j.id, j])),
  };
}

export function toSnapshot(d: Data): Snapshot {
  return { folders: [...d.folders.values()], tasks: [...d.tasks.values()], dependencies: [...d.deps.values()], ai_jobs: [...d.jobs.values()] };
}

export function copy(d: Data): Data {
  return { folders: new Map(d.folders), tasks: new Map(d.tasks), deps: new Map(d.deps), jobs: new Map(d.jobs) };
}

/** Sets each changed entity to how the effect left it. */
export function replay(d: Data, effects: Effect[]) {
  for (const e of effects) set(d, e, "after");
}

/** Puts each changed entity back to how it was before, newest change first. */
export function revert(d: Data, effects: Effect[]) {
  for (const e of [...effects].reverse()) set(d, e, "before");
}

function set(d: Data, e: Effect, side: "before" | "after") {
  switch (e.kind) {
    case "folder": {
      const [id, v] = [(e.before ?? e.after)!.id, e[side]];
      if (v) d.folders.set(id, v);
      else d.folders.delete(id);
      return;
    }
    case "task": {
      const [id, v] = [(e.before ?? e.after)!.id, e[side]];
      if (v) d.tasks.set(id, v);
      else d.tasks.delete(id);
      return;
    }
    case "dependency": {
      const dep = (e.before ?? e.after)!;
      const [key, v] = [depKey(dep.before, dep.after), e[side]];
      if (v) d.deps.set(key, v);
      else d.deps.delete(key);
      return;
    }
    case "ai_job": {
      const [id, v] = [(e.before ?? e.after)!.id, e[side]];
      if (v) d.jobs.set(id, v);
      else d.jobs.delete(id);
    }
  }
}

/** Why the server would turn the operations down, in its words. */
export class Rejected extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export interface Context {
  now: number;
  /** What an earlier mutation changed, for undoing it; `undefined` when this copy does not know. */
  effectsOf(id: MutationId): Effect[] | undefined;
}

/** Applies the operations to `d` and returns what changed. On failure `d` is left as it was. */
export function apply(d: Data, ops: Op[], cx: Context): Effect[] {
  const a = new Applier(d, cx);
  try {
    for (const op of ops) a.op(op);
  } catch (e) {
    revert(d, a.effects);
    throw e;
  }
  return a.effects;
}

class Applier {
  readonly effects: Effect[] = [];

  constructor(
    private readonly d: Data,
    private readonly cx: Context,
  ) {}

  op(op: Op) {
    const now = this.cx.now;
    switch (op.op) {
      case "folder.create": {
        if (this.d.folders.has(op.id)) throw new Rejected("conflict", "文件夹 ID 已存在");
        const parent = op.parent_id ?? null;
        if (parent) this.folder(parent);
        const keys = [...this.d.folders.values()].filter((f) => f.parent_id === parent).map((f) => f.sort_key);
        const sort_key = op.sort_key ?? lastKey(keys);
        this.createFolder({ id: op.id, parent_id: parent, name: op.name, sort_key, created_at: now, updated_at: now });
        return;
      }
      case "folder.update": {
        const before = this.folder(op.id);
        const f = { ...before };
        if (op.name !== undefined) f.name = op.name;
        if (op.parent_id !== undefined) f.parent_id = op.parent_id;
        if (op.sort_key !== undefined) f.sort_key = op.sort_key;
        this.updateFolder(before, f);
        return;
      }
      case "folder.delete":
        this.folder(op.id);
        this.deleteFolder(op.id);
        return;
      case "task.create": {
        if (this.d.tasks.has(op.id)) throw new Rejected("conflict", "事项 ID 已存在");
        const folder = op.folder_id ?? null;
        if (folder) this.folder(folder);
        const keys = [...this.d.tasks.values()].filter((t) => t.folder_id === folder).map((t) => t.sort_key);
        this.createTask({
          id: op.id,
          number: 0,
          folder_id: folder,
          title: op.title,
          description: op.description ?? "",
          done_at: op.done ? now : null,
          due: op.due ?? null,
          sort_key: op.sort_key ?? lastKey(keys),
          created_at: now,
          updated_at: now,
          created_via: null,
          updated_via: null,
        });
        return;
      }
      case "task.update": {
        const before = this.task(op.id);
        const t = { ...before };
        if (op.folder_id !== undefined) t.folder_id = op.folder_id;
        if (op.title !== undefined) t.title = op.title;
        if (op.description !== undefined) t.description = op.description;
        if (op.due !== undefined) t.due = op.due;
        if (op.sort_key !== undefined) t.sort_key = op.sort_key;
        if (op.done !== undefined && op.done !== (t.done_at !== null)) t.done_at = op.done ? now : null;
        this.updateTask(before, t);
        return;
      }
      case "task.delete":
        this.task(op.id);
        this.deleteTask(op.id);
        return;
      case "dep.add":
        if (op.before === op.after) throw new Rejected("invalid", "事项不能依赖自己");
        this.task(op.before);
        this.task(op.after);
        if (this.d.deps.has(depKey(op.before, op.after))) throw new Rejected("conflict", "该依赖已存在");
        this.createDependency({ before: op.before, after: op.after, created_at: now });
        return;
      case "dep.remove": {
        const dep = this.d.deps.get(depKey(op.before, op.after));
        if (!dep) throw new Rejected("not_found", "该依赖不存在");
        this.deleteDependency(dep);
        return;
      }
      case "ai_job.create": {
        if (this.d.jobs.has(op.id)) throw new Rejected("conflict", "请求 ID 已存在");
        const folder = op.folder_id ?? null;
        if (folder) this.folder(folder);
        this.createJob({ id: op.id, folder_id: folder, text: op.text, local_time: op.local_time, state: "waiting", error: null, created_at: now, updated_at: now });
        return;
      }
      case "ai_job.retry": {
        const before = this.job(op.id);
        if (before.state === "running") throw new Rejected("conflict", "正在整理，稍后再改");
        if (before.state === "waiting") throw new Rejected("conflict", "这条请求已在等待整理");
        const j: AiJob = { ...before, state: "waiting", error: null };
        if (op.text !== undefined) j.text = op.text;
        if (op.folder_id !== undefined) {
          if (op.folder_id) this.folder(op.folder_id);
          j.folder_id = op.folder_id;
        }
        this.updateJob(before, j);
        return;
      }
      case "ai_job.delete":
        this.push({ kind: "ai_job", before: this.job(op.id), after: null });
        return;
      case "undo":
        this.undo(op.mutation);
        return;
    }
  }

  private folder(id: FolderId): Folder {
    const f = this.d.folders.get(id);
    if (!f) throw new Rejected("not_found", "文件夹不存在");
    return f;
  }

  private task(id: TaskId): Task {
    const t = this.d.tasks.get(id);
    if (!t) throw new Rejected("not_found", "事项不存在");
    return t;
  }

  private job(id: AiJobId): AiJob {
    const j = this.d.jobs.get(id);
    if (!j) throw new Rejected("not_found", "AI 整理请求不存在");
    return j;
  }

  private createJob(j: AiJob) {
    this.push({ kind: "ai_job", before: null, after: j });
  }

  private updateJob(before: AiJob, after: AiJob) {
    if (same(before, after)) return;
    this.push({ kind: "ai_job", before, after: { ...after, updated_at: this.cx.now } });
  }

  private push(e: Effect) {
    this.effects.push(e);
    set(this.d, e, "after");
  }

  private checkName(f: Folder) {
    for (const o of this.d.folders.values()) {
      if (o.id !== f.id && o.parent_id === f.parent_id && o.name === f.name) {
        throw new Rejected("conflict", `同一层级已有名为“${f.name}”的文件夹`);
      }
    }
  }

  private createFolder(f: Folder) {
    this.checkName(f);
    this.push({ kind: "folder", before: null, after: f });
  }

  private updateFolder(before: Folder, after: Folder) {
    if (same(before, after)) return;
    if (after.parent_id !== before.parent_id && after.parent_id !== null) {
      this.folder(after.parent_id);
      for (let cur: FolderId | null = after.parent_id; cur !== null; cur = this.d.folders.get(cur)!.parent_id) {
        if (cur === after.id) throw new Rejected("folder_cycle", "文件夹不能移入自身或其子文件夹");
      }
    }
    if (after.parent_id !== before.parent_id || after.name !== before.name) this.checkName(after);
    this.push({ kind: "folder", before, after: { ...after, updated_at: this.cx.now } });
  }

  /** Removes everything in the subtree: dependencies, tasks, then folders deepest first, so that
   * undo brings it all back in reverse order. */
  private deleteFolder(id: FolderId) {
    const depth = new Map<FolderId, number>([[id, 0]]);
    for (let grew = true; grew; ) {
      grew = false;
      for (const f of this.d.folders.values()) {
        if (!depth.has(f.id) && f.parent_id !== null && depth.has(f.parent_id)) {
          depth.set(f.id, depth.get(f.parent_id)! + 1);
          grew = true;
        }
      }
    }
    const tasks = [...this.d.tasks.values()].filter((t) => t.folder_id !== null && depth.has(t.folder_id));
    const ids = new Set(tasks.map((t) => t.id));
    for (const dep of [...this.d.deps.values()].filter((dep) => ids.has(dep.before) || ids.has(dep.after))) {
      this.push({ kind: "dependency", before: dep, after: null });
    }
    for (const t of tasks) this.push({ kind: "task", before: t, after: null });
    for (const j of [...this.d.jobs.values()].filter((j) => j.folder_id !== null && depth.has(j.folder_id))) {
      this.push({ kind: "ai_job", before: j, after: null });
    }
    const folders = [...depth].sort((a, b) => b[1] - a[1]).map(([f]) => this.d.folders.get(f)!);
    for (const f of folders) this.push({ kind: "folder", before: f, after: null });
  }

  private createTask(t: Task) {
    this.push({ kind: "task", before: null, after: t });
  }

  private updateTask(before: Task, after: Task) {
    if (same(before, after)) return;
    if (after.folder_id !== before.folder_id && after.folder_id !== null) this.folder(after.folder_id);
    this.push({ kind: "task", before, after: { ...after, updated_at: this.cx.now, updated_via: null } });
  }

  private deleteTask(id: TaskId) {
    for (const dep of [...this.d.deps.values()].filter((dep) => dep.before === id || dep.after === id)) {
      this.push({ kind: "dependency", before: dep, after: null });
    }
    this.push({ kind: "task", before: this.task(id), after: null });
  }

  private createDependency(dep: Dependency) {
    const path = this.chain(dep.after, dep.before);
    if (path) {
      const titles = [dep.before, ...path].map((id) => `“${this.task(id).title}”`).join(" → ");
      throw new Rejected("cycle", `添加该依赖将形成循环：${titles}`);
    }
    this.push({ kind: "dependency", before: null, after: dep });
  }

  private deleteDependency(dep: Dependency) {
    this.push({ kind: "dependency", before: dep, after: null });
  }

  /** A shortest chain of dependencies from `from` to `to`. */
  private chain(from: TaskId, to: TaskId): TaskId[] | undefined {
    const next = new Map<TaskId, TaskId[]>();
    for (const dep of this.d.deps.values()) next.set(dep.before, [...(next.get(dep.before) ?? []), dep.after]);
    const prev = new Map<TaskId, TaskId | null>([[from, null]]);
    const queue = [from];
    for (let i = 0; i < queue.length; i++) {
      if (queue[i] === to) {
        const path = [];
        for (let n: TaskId | null = to; n !== null; n = prev.get(n)!) path.unshift(n);
        return path;
      }
      for (const n of next.get(queue[i]) ?? []) {
        if (!prev.has(n)) {
          prev.set(n, queue[i]);
          queue.push(n);
        }
      }
    }
    return undefined;
  }

  /** Restores what a mutation changed, newest change first, as the server does: removed entities
   * come back unchanged; changes whose target is already gone or back are skipped; bringing
   * something back into a folder or onto a task that is gone fails. */
  private undo(id: MutationId) {
    const effects = this.cx.effectsOf(id);
    if (!effects) throw new Rejected("not_found", "要撤销的改动不存在或已过期");
    const now = this.cx.now;
    for (const e of [...effects].reverse()) {
      if (e.kind === "folder") {
        if (!e.before) {
          if (this.d.folders.has(e.after!.id)) this.deleteFolder(e.after!.id);
        } else if (!e.after) {
          const f = e.before;
          if (this.d.folders.has(f.id)) continue;
          if (f.parent_id !== null && !this.d.folders.has(f.parent_id)) {
            throw new Rejected("not_found", `文件夹“${f.name}”原所在的文件夹已删除`);
          }
          this.createFolder({ ...f, updated_at: now });
        } else {
          const cur = this.d.folders.get(e.before.id);
          if (cur) this.updateFolder(cur, restore(cur, e.before, e.after, ["parent_id", "name", "sort_key"]));
        }
      } else if (e.kind === "task") {
        if (!e.before) {
          if (this.d.tasks.has(e.after!.id)) this.deleteTask(e.after!.id);
        } else if (!e.after) {
          const t = e.before;
          if (this.d.tasks.has(t.id)) continue;
          if (t.folder_id !== null && !this.d.folders.has(t.folder_id)) {
            throw new Rejected("not_found", `“${t.title}”原所在的文件夹已删除`);
          }
          this.createTask({ ...t, updated_at: now, updated_via: null });
        } else {
          const cur = this.d.tasks.get(e.before.id);
          const fields = ["folder_id", "title", "description", "done_at", "due", "sort_key"] as const;
          if (cur) this.updateTask(cur, restore(cur, e.before, e.after, fields));
        }
      } else if (e.kind === "ai_job") {
        if (!e.before) {
          if (this.d.jobs.has(e.after!.id)) this.push({ kind: "ai_job", before: this.job(e.after!.id), after: null });
        } else if (!e.after) {
          const j = e.before;
          if (this.d.jobs.has(j.id)) continue;
          if (j.folder_id !== null && !this.d.folders.has(j.folder_id)) throw new Rejected("not_found", "AI 整理请求原所在的文件夹已删除");
          // What it was being organized into was thrown away, or is undone now.
          const back: AiJob = j.state === "running" ? { ...j, state: "failed", error: "已撤销" } : j;
          this.createJob({ ...back, updated_at: now });
        }
        // Starting, failing and retrying are the job's own course, not undone.
      } else if (!e.before) {
        const dep = this.d.deps.get(depKey(e.after!.before, e.after!.after));
        if (dep) this.deleteDependency(dep);
      } else if (!e.after) {
        const dep = e.before;
        if (!this.d.tasks.has(dep.before) || !this.d.tasks.has(dep.after)) {
          throw new Rejected("not_found", "要恢复的依赖所关联的事项已删除");
        }
        if (!this.d.deps.has(depKey(dep.before, dep.after))) this.createDependency(dep);
      }
    }
  }
}

/** A key after everything in `keys`, for something new at the end. */
export function lastKey(keys: string[]): string {
  return generateKeyBetween(keys.reduce<string | null>((max, k) => (max === null || cmp(k, max) > 0 ? k : max), null), null);
}

/** `cur` with the fields that changed from `before` to `after` set back to `before`, so that undoing
 * a change leaves later changes to other fields alone. */
function restore<T extends object>(cur: T, before: T, after: T, fields: readonly (keyof T)[]): T {
  const restored = { ...cur };
  for (const f of fields) if (!same(before[f], after[f])) restored[f] = before[f];
  return restored;
}

/** Equal as data: entities and their fields are plain JSON. */
function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
