// Applies operations to a local copy the way the server does (`crates/server/src/mutate/`), and
// yields the same kind of effects, so the app shows a change at once and can undo it locally.
// What only the server knows stays unknown: a new task's number, or that of one moved to someone
// else's, is 0 until the server's copy arrives. Sharing and leaving a folder go to the server
// directly (`views/share.tsx`), as the people to add are only known there.

import { generateNKeysBetween } from "fractional-indexing";
import type { AiJob } from "../api/types/AiJob";
import type { AiJobId } from "../api/types/AiJobId";
import type { Attachment } from "../api/types/Attachment";
import type { AttachmentId } from "../api/types/AttachmentId";
import type { Completion } from "../api/types/Completion";
import type { Dependency } from "../api/types/Dependency";
import type { Due } from "../api/types/Due";
import type { Effect } from "../api/types/Effect";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Mutation } from "../api/types/Mutation";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { Person } from "../api/types/Person";
import type { Remind } from "../api/types/Remind";
import type { Share } from "../api/types/Share";
import type { Snapshot } from "../api/types/Snapshot";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { type Now, cmp, comesRound, holdsUp } from "./board";
import { MAX_REMINDERS, startInOrder, timesInOrder } from "./dates";
import { lunarDate } from "./lunar";
import { nextDue, occurrence, plus, shifted } from "./repeat";

/** What a `task.update` changes. */
export type TaskFields = Omit<Extract<Op, { op: "task.update" }>, "op" | "id">;

/** A user's data, keyed for changes. */
export interface Data {
  folders: Map<FolderId, Folder>;
  tasks: Map<TaskId, Task>;
  deps: Map<string, Dependency>;
  completions: Map<string, Completion>;
  attachments: Map<AttachmentId, Attachment>;
  jobs: Map<AiJobId, AiJob>;
  shares: Map<FolderId, Share>;
  people: Map<string, Person>;
}

/** What one change may hold, as `MAX_OPS` in crates/core/src/ops.rs has it. */
export const MAX_OPS = 1000;

export const depKey = (before: TaskId, after: TaskId) => `${before}>${after}`;
export const completionKey = (c: Pick<Completion, "task_id" | "due" | "again">) => `${c.task_id} ${c.due} ${c.again}`;

export function fromSnapshot(s: Snapshot): Data {
  return {
    folders: new Map(s.folders.map((f) => [f.id, f])),
    tasks: new Map(s.tasks.map((t) => [t.id, t])),
    deps: new Map(s.dependencies.map((d) => [depKey(d.before, d.after), d])),
    completions: new Map(s.completions.map((c) => [completionKey(c), c])),
    attachments: new Map(s.attachments.map((a) => [a.id, a])),
    jobs: new Map(s.ai_jobs.map((j) => [j.id, j])),
    shares: new Map(s.shares.map((sh) => [sh.folder_id, sh])),
    people: new Map(s.people.map((p) => [p.id, p])),
  };
}

export function toSnapshot(d: Data): Snapshot {
  return {
    folders: [...d.folders.values()],
    tasks: [...d.tasks.values()],
    dependencies: [...d.deps.values()],
    completions: [...d.completions.values()],
    attachments: [...d.attachments.values()],
    ai_jobs: [...d.jobs.values()],
    shares: [...d.shares.values()],
    people: [...d.people.values()],
  };
}

export function copy(d: Data): Data {
  return {
    folders: new Map(d.folders),
    tasks: new Map(d.tasks),
    deps: new Map(d.deps),
    completions: new Map(d.completions),
    attachments: new Map(d.attachments),
    jobs: new Map(d.jobs),
    shares: new Map(d.shares),
    people: new Map(d.people),
  };
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
    case "completion": {
      const [key, v] = [completionKey((e.before ?? e.after)!), e[side]];
      if (v) d.completions.set(key, v);
      else d.completions.delete(key);
      return;
    }
    case "attachment": {
      const [id, v] = [(e.before ?? e.after)!.id, e[side]];
      if (v) d.attachments.set(id, v);
      else d.attachments.delete(id);
      return;
    }
    case "ai_job": {
      const [id, v] = [(e.before ?? e.after)!.id, e[side]];
      if (v) d.jobs.set(id, v);
      else d.jobs.delete(id);
      return;
    }
    case "share": {
      const [id, v] = [(e.before ?? e.after)!.folder_id, e[side]];
      if (v) d.shares.set(id, v);
      else d.shares.delete(id);
      return;
    }
    case "person": {
      const [id, v] = [(e.before ?? e.after)!.id, e[side]];
      if (v) d.people.set(id, v);
      else d.people.delete(id);
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
  /** The user making the change. */
  me: string;
  now: number;
  /** The day and time on the device that made it. */
  local: Now;
  /** What an earlier mutation changed, for undoing it; `undefined` when this copy does not know. */
  effectsOf(id: MutationId): Effect[] | undefined;
  /** What the changes after mutation `id`, up to the one being made, did, as far as this copy knows. */
  effectsAfter(id: MutationId): Effect[][];
}

/** The day and time on the device that made `m`, as the server takes them (`mutate::local`): as given
 * with it, or with a task done in it, or else UTC's at `at`. */
export function localOf(m: Mutation, at: number): Now {
  if (m.today) return { today: m.today, ...(m.time && { time: m.time }) };
  for (const op of m.ops) if (op.op === "task.update" && op.today) return { today: op.today };
  const utc = new Date(at).toISOString();
  return { today: utc.slice(0, 10), time: utc.slice(11, 16) };
}

/** Applies the operations to `d` and returns what changed. On failure `d` is left as it was. */
export function apply(d: Data, ops: Op[], cx: Context): Effect[] {
  const a = new Applier(d, cx);
  try {
    for (const op of ops) a.op(op);
    a.settle();
  } catch (e) {
    revert(d, a.effects);
    throw e;
  }
  return a.effects;
}

/** Where something is, as far as who can see it: the user's own data, or a folder its owner shares. */
interface Space {
  owner: string;
  /** The shared folder. */
  root: FolderId | null;
  members: string[];
}

const sees = (space: Space, user: string) => space.owner === user || space.members.includes(user);

/** The people `users` may be assigned to a task in `space`: some of those it is shared with. */
function checkAssignees(space: Space, users: string[]) {
  if (!users.length) return;
  if (!space.root) throw new Rejected("invalid", "只有共享的文件夹里的事项能设负责人");
  if (!users.every((u) => sees(space, u))) throw new Rejected("invalid", "负责人须是这个文件夹的共享人或成员");
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
        this.createFolder({
          id: op.id,
          parent_id: parent,
          name: op.name,
          sort_key,
          color: op.color ?? null,
          icon: op.icon ?? null,
          created_at: now,
          updated_at: now,
        });
        return;
      }
      case "folder.update": {
        const before = this.folder(op.id);
        const f = { ...before };
        if (op.name !== undefined) f.name = op.name;
        if (op.parent_id !== undefined) f.parent_id = op.parent_id;
        if (op.sort_key !== undefined) f.sort_key = op.sort_key;
        if (op.color !== undefined) f.color = op.color;
        if (op.icon !== undefined) f.icon = op.icon;
        this.updateFolder(before, f);
        return;
      }
      case "folder.delete":
        this.folder(op.id);
        this.deleteFolder(op.id);
        return;
      case "task.create": {
        if (this.d.tasks.has(op.id)) throw new Rejected("conflict", "事项 ID 已存在");
        if (op.due && op.due_after) throw bothDue();
        const folder = op.folder_id ?? null;
        if (folder) this.folder(folder);
        const keys = [...this.d.tasks.values()].filter((t) => t.folder_id === folder).map((t) => t.sort_key);
        const assignees = [...new Set(op.assignees)].sort();
        checkAssignees(this.space(folder), assignees);
        this.createTask({
          id: op.id,
          number: 0,
          folder_id: folder,
          title: op.title,
          description: op.description ?? "",
          done_at: op.done ? now : null,
          dropped: false,
          start: op.start ?? null,
          start_time: op.start_time,
          start_reminders: byStart(op.start_time, op.start_reminders ?? []),
          remind_at: listed(timesInOrder(op.remind_at ?? [])),
          due_after: op.due_after,
          due: op.due ?? null,
          sort_key: op.sort_key ?? lastKey(keys),
          assignees,
          created_at: now,
          updated_at: now,
          created_by: this.cx.me,
          updated_by: this.cx.me,
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
        if (op.start !== undefined) {
          // The time goes with the date; a new date keeps it.
          if (op.start === null) t.start_time = undefined;
          t.start = op.start;
        }
        if (op.start_time !== undefined) t.start_time = op.start_time ?? undefined;
        t.start_reminders = op.start_reminders ? byStart(t.start_time, op.start_reminders) : fitStart(t);
        if (op.remind_at) t.remind_at = listed(timesInOrder(op.remind_at));
        if (op.due !== undefined && op.due_after !== undefined) throw bothDue();
        if (op.due !== undefined) {
          // As given, before doing it the last time could take the repeat away.
          checkRepeat(t.title, op.due);
          t.due = op.due;
          t.due_after = undefined;
          t.unlocked_on = undefined;
        }
        if (op.due_after !== undefined) {
          // As in mutate/ops.rs: turned to or from counting after the prerequisites, the due date goes.
          if (!t.due_after !== (op.due_after === null)) {
            t.unlocked_on = undefined;
            t.due = null;
          }
          t.due_after = op.due_after ?? undefined;
        }
        if (op.sort_key !== undefined) t.sort_key = op.sort_key;
        if (op.assign?.length || op.unassign?.length) {
          if (t.folder_id !== before.folder_id && t.folder_id) this.folder(t.folder_id);
          checkAssignees(this.space(t.folder_id), op.assign ?? []);
          t.assignees = [...new Set([...t.assignees, ...(op.assign ?? [])])].filter((u) => !op.unassign?.includes(u)).sort();
        }
        let record: Completion | undefined;
        // Done from a list behind the task: that time of a repeating one is done already, or gone
        // (docs/api.md#改动).
        const time = occurrence(t);
        const behind = !!op.occurrence && !!time && (op.occurrence.due !== time.due || op.occurrence.completions !== time.completions);
        // Done once given up, it was done after all, ending when it did; not a time of a repeating one
        // a list behind showed, which giving it up ended (docs/api.md#改动).
        if (op.done && t.dropped) {
          if (!op.occurrence) t.dropped = false;
        }
        else if (op.done !== undefined && op.done !== (t.done_at !== null) && !(op.done && behind)) {
          t.dropped = false;
          const repeat = t.due?.repeat;
          // Done, a repeating task stays open, due next time; done the last time, it is done
          // (docs/design.md#重复).
          if (op.done && t.due && repeat) {
            if (!op.today) throw new Rejected("invalid", "完成重复的事项需要给出今天的日期");
            record = { task_id: t.id, due: t.due.date, done: op.today, again: this.nextAgain(t.id, t.due.date) };
            const date = nextDue(repeat, t.due.date, op.today);
            if (date) {
              // The start date keeps as far before it, at the same time (docs/design.md#开始时间).
              if (t.start) t.start = shifted(t.start, t.due.date, date);
              t.due = { ...t.due, date };
            } else {
              const { repeat: ended, ...rest } = t.due;
              t.due = rest;
              t.done_at = now;
            }
            t.completions = { count: (t.completions?.count ?? 0) + 1, last: op.today };
          } else t.done_at = op.done ? now : null;
        }
        this.updateTask(before, t);
        if (record) this.recordCompletion(record);
        return;
      }
      case "task.drop": {
        const before = this.task(op.id);
        // Done already, it ends given up as it did; for good, with no next time (docs/design.md#放弃).
        const t = { ...before, done_at: before.done_at ?? now, dropped: true };
        if (t.due?.repeat) {
          const { repeat, ...rest } = t.due;
          t.due = rest;
        }
        this.updateTask(before, t);
        return;
      }
      case "task.delete":
        this.task(op.id);
        this.deleteTask(op.id);
        return;
      case "dep.add":
        if (op.before === op.after) throw new Rejected("invalid", "事项不能依赖自己");
        if (!this.together(op.before, op.after)) {
          throw new Rejected("invalid", "只能连同一处的事项：私人的事项之间，或同一个共享的文件夹里的事项之间");
        }
        if (this.d.deps.has(depKey(op.before, op.after))) throw new Rejected("conflict", "该依赖已存在");
        this.createDependency({ before: op.before, after: op.after, created_at: now });
        return;
      case "dep.remove": {
        const dep = this.d.deps.get(depKey(op.before, op.after));
        if (!dep) throw new Rejected("not_found", "该依赖不存在");
        this.deleteDependency(dep);
        return;
      }
      case "attachment.add": {
        // Whether its file is there, and is the user's, only the server knows.
        if (this.d.attachments.has(op.id)) throw new Rejected("conflict", "附件 ID 已存在");
        if ((op.task_id === undefined) === (op.job_id === undefined)) {
          throw new Rejected("invalid", "附件要属于一个事项或一条 AI 整理请求：task_id 和 job_id 给且只给一个");
        }
        const a: Attachment = {
          id: op.id,
          task_id: op.task_id ?? null,
          job_id: op.job_id ?? null,
          name: op.name,
          type: op.type,
          size: op.size,
          created_at: now,
        };
        this.holder(a);
        this.push({ kind: "attachment", before: null, after: a });
        return;
      }
      case "attachment.update": {
        const before = this.attachment(op.id);
        const a = { ...before };
        if (op.name !== undefined) a.name = op.name;
        if (op.task_id !== undefined) [a.task_id, a.job_id] = [op.task_id, null];
        this.updateAttachment(before, a);
        return;
      }
      case "attachment.delete":
        this.deleteAttachment(this.attachment(op.id));
        return;
      case "ai_job.create": {
        if (this.d.jobs.has(op.id)) throw new Rejected("conflict", "请求 ID 已存在");
        const folder = op.folder_id ?? null;
        if (folder) this.folder(folder);
        this.createJob({
          id: op.id,
          folder_id: folder,
          kind: op.kind ?? "organize",
          text: op.text,
          local_time: op.local_time,
          ...(op.reminders && { reminders: op.reminders }),
          state: "waiting",
          error: null,
          suggestions: null,
          created_at: now,
          updated_at: now,
        });
        return;
      }
      case "ai_job.retry": {
        const before = this.job(op.id);
        if (before.state === "running") throw new Rejected("conflict", "正在整理，稍后再改");
        if (before.state === "waiting") throw new Rejected("conflict", "这条请求已在等待整理");
        const { reminders: _, ...kept } = before;
        const j: AiJob = { ...kept, state: "waiting", error: null, suggestions: null, ...(op.reminders && { reminders: op.reminders }) };
        if (op.text !== undefined) j.text = op.text;
        if (op.folder_id !== undefined) {
          if (op.folder_id) this.folder(op.folder_id);
          j.folder_id = op.folder_id;
        }
        this.updateJob(before, j);
        return;
      }
      case "ai_job.delete":
        this.deleteJob(this.job(op.id));
        return;
      case "share.add":
      case "share.remove":
        throw new Rejected("offline", "添加、移除成员需要联网");
      case "undo":
        this.undo(op.mutation);
        return;
    }
  }

  /** Whether tasks `a` and `b` are in the same place, where they can be linked: both private, or in
   * the same shared folder. */
  private together(a: TaskId, b: TaskId): boolean {
    return this.root(this.task(a).folder_id) === this.root(this.task(b).folder_id);
  }

  /** The shared folder `f` is in, or is. */
  private root(f: FolderId | null): FolderId | null {
    for (let cur = f; cur !== null; cur = this.d.folders.get(cur)!.parent_id) if (this.d.shares.has(cur)) return cur;
    return null;
  }

  /** Where what is in folder `f` is. */
  private space(f: FolderId | null): Space {
    const root = this.root(f);
    const share = root ? this.d.shares.get(root)! : undefined;
    return { owner: share?.owner ?? this.cx.me, root, members: share?.members ?? [] };
  }

  /** The folders in the subtree rooted at `id`, it first, each before its subfolders. */
  private subtree(id: FolderId): Folder[] {
    const list = [this.d.folders.get(id)!];
    for (let i = 0; i < list.length; i++) for (const f of this.d.folders.values()) if (f.parent_id === list[i].id) list.push(f);
    return list;
  }

  private subtreeTasks(id: FolderId): Task[] {
    const inside = new Set(this.subtree(id).map((f) => f.id));
    return [...this.d.tasks.values()].filter((t) => t.folder_id !== null && inside.has(t.folder_id));
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

  private attachment(id: AttachmentId): Attachment {
    const a = this.d.attachments.get(id);
    if (!a) throw new Rejected("not_found", "附件不存在");
    return a;
  }

  /** What `a` belongs to, which has to be there. */
  private holder(a: Attachment) {
    if (a.task_id !== null) this.task(a.task_id);
    else this.job(a.job_id!);
  }

  private updateAttachment(before: Attachment, after: Attachment) {
    if (same(before, after)) return;
    if (after.task_id !== before.task_id || after.job_id !== before.job_id) this.holder(after);
    this.push({ kind: "attachment", before, after });
  }

  private deleteAttachment(a: Attachment) {
    this.push({ kind: "attachment", before: a, after: null });
  }

  private deleteJob(j: AiJob) {
    for (const a of [...this.d.attachments.values()]) if (a.job_id === j.id) this.deleteAttachment(a);
    this.push({ kind: "ai_job", before: j, after: null });
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

  /** As `Cx::settle` in mutate/mod.rs: of the tasks due some days after their prerequisites are done that
   * the change concerns, gives those with all of them done the day it was made, and takes that back
   * from those a prerequisite came to hold up in it, but for a repeating one done before, its time come
   * round again (docs/design.md#前置完成后的截止). */
  settle() {
    const local = this.cx.local;
    const concerned = new Set<TaskId>();
    // A prerequisite that came to hold up a task, and the task, as `depKey` has them.
    const held = new Set<string>();
    // Tasks that came to hold up those after them, or stopped.
    const turned: [TaskId, boolean][] = [];
    for (const e of this.effects) {
      if (e.kind === "task" && e.after) {
        if (e.after.due_after) concerned.add(e.after.id);
        const holds = holdsUp(e.after, local);
        if (e.before && holds !== holdsUp(e.before, local)) turned.push([e.after.id, holds]);
      } else if (e.kind === "dependency") {
        const dep = (e.before ?? e.after)!;
        concerned.add(dep.after);
        if (e.after) held.add(depKey(dep.before, dep.after));
      }
    }
    for (const [id, holds] of turned) {
      for (const dep of this.d.deps.values()) {
        if (dep.before !== id) continue;
        concerned.add(dep.after);
        if (holds) held.add(depKey(dep.before, dep.after));
      }
    }
    for (const id of concerned) {
      const t = this.d.tasks.get(id);
      if (!t?.due_after) continue;
      const holding = [...this.d.deps.values()].filter((dep) => dep.after === id && holdsUp(this.d.tasks.get(dep.before)!, local));
      if (!t.unlocked_on && !holding.length) this.updateTask(t, { ...t, unlocked_on: local.today });
      else if (t.unlocked_on && holding.some((dep) => held.has(depKey(dep.before, id)) && !comesRound(this.d.tasks.get(dep.before)!))) {
        this.updateTask(t, { ...t, unlocked_on: undefined });
      }
    }
  }

  private checkName(f: Folder) {
    for (const o of this.d.folders.values()) {
      // Folders others share are at the top of no tree of the user's.
      const elsewhere = this.d.shares.get(o.id)?.owner !== undefined && this.d.shares.get(o.id)!.owner !== this.cx.me;
      if (o.id !== f.id && o.parent_id === f.parent_id && o.name === f.name && !elsewhere) {
        throw new Rejected("conflict", `同一层级已有名为“${f.name}”的文件夹`);
      }
    }
  }

  private createFolder(f: Folder) {
    this.checkName(f);
    this.push({ kind: "folder", before: null, after: f });
  }

  /** Changes a folder. One moved into or out of a shared folder takes along everything in it; its
   * tasks lose the dependencies on what stays behind, and the assignees who can't see them now. */
  private updateFolder(before: Folder, after: Folder) {
    if (same(before, after)) return;
    const from = this.space(before.id);
    const shared = from.root === before.id;
    if (shared && from.owner !== this.cx.me) throw new Rejected("forbidden", "只有共享人能改名、移动或删除共享的文件夹");
    let to = from;
    if (after.parent_id !== before.parent_id && after.parent_id !== null) {
      this.folder(after.parent_id);
      for (let cur: FolderId | null = after.parent_id; cur !== null; cur = this.d.folders.get(cur)!.parent_id) {
        if (cur === after.id) throw new Rejected("folder_cycle", "文件夹不能移入自身或其子文件夹");
      }
    }
    if (after.parent_id !== before.parent_id) {
      const there = this.space(after.parent_id);
      if (there.root && this.subtree(before.id).some((f) => this.d.shares.has(f.id))) {
        throw new Rejected("invalid", "共享的文件夹不能放进另一个共享的文件夹");
      }
      // A shared folder takes its space along.
      if (!shared) to = there;
    }
    if (after.parent_id !== before.parent_id || after.name !== before.name) this.checkName(after);
    if (from.root !== to.root) {
      this.cutOff(before.id);
      this.carry(before.id, from, to);
    }
    this.push({ kind: "folder", before, after: { ...after, updated_at: this.cx.now } });
  }

  /** Removes the dependencies between the tasks in the subtree rooted at `id` and those outside it. */
  private cutOff(id: FolderId) {
    const inside = new Set(this.subtreeTasks(id).map((t) => t.id));
    for (const dep of [...this.d.deps.values()]) {
      if (inside.has(dep.before) !== inside.has(dep.after)) this.deleteDependency(dep);
    }
  }

  /** Makes the tasks in the subtree rooted at `id`, moved from `from` to `to`, belong there: to `to`'s
   * owner, numbered anew if that is someone else, assigned only to people there. */
  private carry(id: FolderId, from: Space, to: Space) {
    for (const t of this.subtreeTasks(id)) {
      const after = { ...t, assignees: t.assignees.filter((u) => to.root && sees(to, u)) };
      if (to.owner !== from.owner) after.number = 0;
      if (!same(t, after)) this.push({ kind: "task", before: t, after });
    }
  }

  /** Removes everything in the subtree: dependencies, attachments, completion records, tasks, AI jobs, then folders deepest first, so that
   * undo brings it all back in reverse order. Shared folders inside go first. A shared folder stops
   * being shared first, its tasks assigned to nobody before, so that bringing it back shares it again
   * and then assigns them again. */
  private deleteFolder(id: FolderId) {
    const share = this.d.shares.get(id);
    if (share) {
      if (share.owner !== this.cx.me) throw new Rejected("forbidden", "只有共享人能改名、移动或删除共享的文件夹");
      for (const t of this.subtreeTasks(id)) if (t.assignees.length) this.push({ kind: "task", before: t, after: { ...t, assignees: [] } });
      this.push({ kind: "share", before: share, after: null });
    } else if (!this.root(id)) {
      for (const f of this.subtree(id)) if (this.d.shares.has(f.id)) this.deleteFolder(f.id);
    }
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
    for (const a of [...this.d.attachments.values()].filter((a) => a.task_id !== null && ids.has(a.task_id))) this.deleteAttachment(a);
    for (const c of [...this.d.completions.values()]) if (ids.has(c.task_id)) this.deleteCompletion(c);
    for (const t of tasks) this.push({ kind: "task", before: t, after: null });
    for (const j of [...this.d.jobs.values()].filter((j) => j.folder_id !== null && depth.has(j.folder_id))) this.deleteJob(j);
    const folders = [...depth].sort((a, b) => b[1] - a[1]).map(([f]) => this.d.folders.get(f)!);
    for (const f of folders) this.push({ kind: "folder", before: f, after: null });
  }

  private createTask(task: Task) {
    const t = dueAfter(task);
    checkTask(t);
    const space = this.space(t.folder_id);
    this.push({ kind: "task", before: null, after: { ...t, assignees: t.assignees.filter((u) => space.root && sees(space, u)) } });
  }

  /** Changes a task. One moved into or out of a shared folder loses its dependencies and the
   * assignees who can't see it now, and belongs to the owner of where it is. */
  private updateTask(before: Task, changed: Task) {
    const after = dueAfter(changed);
    if (same(before, after)) return;
    checkTask(after);
    if (after.folder_id !== before.folder_id && after.folder_id !== null) this.folder(after.folder_id);
    const [from, to] = [this.space(before.folder_id), this.space(after.folder_id)];
    if (from.root !== to.root) {
      for (const dep of [...this.d.deps.values()]) if (dep.before === before.id || dep.after === before.id) this.deleteDependency(dep);
    }
    const t = { ...after, assignees: after.assignees.filter((u) => to.root && sees(to, u)) };
    // The number the new owner gives it, or gives back, comes with the server's copy.
    if (to.owner !== from.owner) t.number = 0;
    this.push({ kind: "task", before, after: { ...t, updated_at: this.cx.now, updated_by: this.cx.me, updated_via: null } });
  }

  private deleteTask(id: TaskId) {
    for (const dep of [...this.d.deps.values()].filter((dep) => dep.before === id || dep.after === id)) {
      this.push({ kind: "dependency", before: dep, after: null });
    }
    for (const a of [...this.d.attachments.values()]) if (a.task_id === id) this.deleteAttachment(a);
    for (const c of [...this.d.completions.values()]) if (c.task_id === id) this.deleteCompletion(c);
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

  /** The `again` of a new record of the time task `id` was due on `due`: after those it has. */
  private nextAgain(id: TaskId, due: string) {
    let next = 0;
    for (const c of this.d.completions.values()) if (c.task_id === id && c.due === due) next = Math.max(next, c.again + 1);
    return next;
  }

  /** Records a time its task was done. */
  private recordCompletion(c: Completion) {
    this.push({ kind: "completion", before: null, after: c });
  }

  private deleteCompletion(c: Completion) {
    this.push({ kind: "completion", before: c, after: null });
  }

  /** Shares the user's folder `root` with `user` too. The first time, its tasks lose their
   * dependencies on those outside it. */
  private addMember(root: FolderId, user: string) {
    const share = this.d.shares.get(root);
    if (!share) this.cutOff(root);
    const members = [...(share?.members ?? []), user].sort();
    this.push({ kind: "share", before: share ?? null, after: { folder_id: root, owner: this.cx.me, members } });
  }

  /** Takes `user` off the user's shared folder `root`, and off the tasks they were to do. Once nobody
   * is left, it is no longer shared, and nobody is assigned anything. */
  private removeMember(root: FolderId, user: string) {
    const share = this.d.shares.get(root)!;
    const last = share.members.length === 1;
    for (const t of this.subtreeTasks(root)) {
      const assignees = t.assignees.filter((u) => !last && u !== user);
      if (assignees.length !== t.assignees.length) this.push({ kind: "task", before: t, after: { ...t, assignees } });
    }
    const members = share.members.filter((m) => m !== user);
    this.push({ kind: "share", before: share, after: last ? null : { ...share, members } });
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

  /** How often task `task` was done, as each change since changed it, in order, up to this one so
   * far: `rest`, what came after in mutation `id`, then the mutations after it. */
  private completionsSince(id: MutationId, task: TaskId, rest: Effect[]) {
    return [rest, ...this.cx.effectsAfter(id), this.effects]
      .flat()
      .flatMap((e) => (e.kind === "task" && e.before && e.after?.id === task && !same(e.before.completions, e.after.completions) ? [[e.before.completions, e.after.completions] as [Task["completions"], Task["completions"]]] : []));
  }

  /** Restores what a mutation changed, newest change first, as the server does: removed entities
   * come back unchanged; changes whose target is already gone or back are skipped; bringing
   * something back into a folder or onto a task that is gone fails. */
  private undo(id: MutationId) {
    const effects = this.cx.effectsOf(id);
    if (!effects) throw new Rejected("not_found", "要撤销的改动不存在或已过期");
    const now = this.cx.now;
    // A folder it made goes empty, in turn, freeing its name for one it deleted. Holding what was put
    // in it since, it waits for the end: what it made goes in turn before, what else stays, and the
    // folder with that.
    const ours = new Set(effects.flatMap((e) => (e.kind === "folder" && !e.before ? [e.after!.id] : [])));
    // Nothing in it but folders in `only`, and in them the same: no task, no AI job.
    const onlyFoldersOf = (id: FolderId, only: Set<FolderId>) => {
      const inside = this.subtree(id);
      const ids = new Set(inside.map((f) => f.id));
      return inside.every((f) => only.has(f.id)) && ![...this.d.tasks.values(), ...this.d.jobs.values()].some((x) => x.folder_id !== null && ids.has(x.folder_id));
    };
    const made: FolderId[] = [];
    // The tasks this brings back, or sets back to how often they were done: only their records come
    // back, one back from elsewhere having those it came back with. A deletion took the records out
    // before the task, so going backwards the task comes back first; a count is known up front, as a
    // time done again before a time kept a record each had its earlier record taken out after the
    // count went up.
    const counted = new Set(effects.flatMap((e) => (e.kind === "task" && e.before && e.after && !same(e.before.completions, e.after.completions) ? [e.before.id] : [])));
    for (const [i, e] of [...effects.entries()].reverse()) {
      if (e.kind === "folder") {
        if (!e.before) {
          const id = e.after!.id;
          if (this.d.folders.has(id)) {
            if (onlyFoldersOf(id, new Set([id]))) this.deleteFolder(id);
            else made.push(id);
          }
        } else if (!e.after) {
          const f = e.before;
          // Its ID taken again by one it made, which still holds what was put in it since.
          if (made.includes(f.id)) throw new Rejected("conflict", `文件夹“${f.name}”无法恢复：之后又往取代它的文件夹里放了东西，请先移走`);
          if (this.d.folders.has(f.id)) continue;
          if (f.parent_id !== null && !this.d.folders.has(f.parent_id)) {
            throw new Rejected("not_found", `文件夹“${f.name}”原所在的文件夹已删除`);
          }
          this.createFolder({ ...f, updated_at: now });
        } else {
          const cur = this.d.folders.get(e.before.id);
          if (cur) this.updateFolder(cur, restore(cur, e.before, e.after, ["parent_id", "name", "sort_key", "color", "icon"]));
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
          counted.add(t.id);
          this.createTask({ ...t, updated_at: now, updated_by: this.cx.me, updated_via: null });
        } else {
          const cur = this.d.tasks.get(e.before.id);
          const fields = ["folder_id", "title", "description", "done_at", "dropped", "start", "start_time", "start_reminders", "remind_at", "due_after", "unlocked_on", "completions", "sort_key"] as const;
          // Done again or taken back since, maybe to the same count on the same day: setting the count
          // back would undo that too.
          const { before, after } = e;
          if (
            cur &&
            !same(before.completions, after.completions) &&
            (!same(cur.completions, after.completions) || !stillLast(before.completions, after.completions, this.completionsSince(id, cur.id, effects.slice(i + 1))))
          ) {
            throw new Rejected("conflict", `“${cur.title}”在这之后又完成或撤销过，请先撤销后来的那次`);
          }
          if (cur) {
            const restored = restore(cur, e.before, e.after, fields);
            restored.start_time = restored.start ? restored.start_time : undefined;
            // A due date `due_after` gave is given again.
            const set = (t: Task) => (t.due_after ? null : t.due);
            this.updateTask(cur, {
              ...restored,
              // A time without its date, which was cleared since, goes too, and reminders by the start
              // with nothing to go by; so does the day prerequisites were done, without a due date
              // counting from it, and giving up one open since.
              start_reminders: fitStart(restored),
              unlocked_on: restored.due_after ? restored.unlocked_on : undefined,
              dropped: restored.dropped && restored.done_at !== null,
              due: restoreDue(set(cur), set(e.before), set(e.after)),
              assignees: restorePeople(cur.assignees, e.before.assignees, e.after.assignees),
            });
          }
        }
      } else if (e.kind === "attachment") {
        if (!e.before) {
          const cur = this.d.attachments.get(e.after!.id);
          if (cur) this.deleteAttachment(cur);
        } else if (!e.after) {
          const a = e.before;
          if (this.d.attachments.has(a.id)) continue;
          // Whether its file is still kept, only the server knows.
          if (a.task_id !== null ? !this.d.tasks.has(a.task_id) : !this.d.jobs.has(a.job_id!)) {
            throw new Rejected("not_found", `附件“${a.name}”所属的事项已删除`);
          }
          this.push({ kind: "attachment", before: null, after: a });
        } else {
          const cur = this.d.attachments.get(e.before.id);
          // What it belongs to is one thing, in two fields, as the server has it.
          const moved = e.before.task_id !== e.after.task_id || e.before.job_id !== e.after.job_id;
          const holder = moved ? { task_id: e.before.task_id, job_id: e.before.job_id } : {};
          if (cur) this.updateAttachment(cur, { ...restore(cur, e.before, e.after, ["name"]), ...holder });
        }
      } else if (e.kind === "ai_job") {
        if (!e.before) {
          if (this.d.jobs.has(e.after!.id)) this.deleteJob(this.job(e.after!.id));
        } else if (!e.after) {
          const j = e.before;
          if (this.d.jobs.has(j.id)) continue;
          if (j.folder_id !== null && !this.d.folders.has(j.folder_id)) throw new Rejected("not_found", "AI 整理请求原所在的文件夹已删除");
          // What it was being organized into was thrown away, or is undone now.
          const back: AiJob = j.state === "running" ? { ...j, state: "failed", error: "已撤销" } : j;
          this.createJob({ ...back, updated_at: now });
        }
        // Starting, failing and retrying are the job's own course, not undone.
      } else if (e.kind === "share") {
        const { folder_id, owner } = (e.before ?? e.after)!;
        if (owner !== this.cx.me) throw new Rejected("forbidden", "只有共享人能添加或移除成员");
        if (!this.d.folders.has(folder_id)) continue;
        // Those it added go, and those it took off come back, as far as nothing has done so since.
        const [b, a] = [e.before?.members ?? [], e.after?.members ?? []];
        const now = this.d.shares.get(folder_id)?.members ?? [];
        for (const u of a.filter((u) => !b.includes(u) && now.includes(u))) this.removeMember(folder_id, u);
        for (const u of b.filter((u) => !a.includes(u) && !now.includes(u))) this.addMember(folder_id, u);
        // Names are not undone.
      } else if (e.kind === "person") {
        continue;
      } else if (e.kind === "completion") {
        const cur = this.d.completions.get(completionKey((e.before ?? e.after)!));
        if (!e.before) {
          if (cur) this.deleteCompletion(cur);
        } else if (!cur && counted.has(e.before.task_id) && this.d.tasks.has(e.before.task_id)) this.recordCompletion(e.before);
      } else if (!e.before) {
        const dep = this.d.deps.get(depKey(e.after!.before, e.after!.after));
        if (dep) this.deleteDependency(dep);
      } else if (!e.after) {
        const dep = e.before;
        if (!this.d.tasks.has(dep.before) || !this.d.tasks.has(dep.after)) {
          throw new Rejected("not_found", "要恢复的依赖所关联的事项已删除");
        }
        // Moved apart since, they can't be linked again.
        if (!this.d.deps.has(depKey(dep.before, dep.after)) && this.together(dep.before, dep.after)) this.createDependency(dep);
      }
    }
    for (const id of made) if (this.d.folders.has(id) && onlyFoldersOf(id, ours)) this.deleteFolder(id);
  }
}

/** `cur` with the people added between `before` and `after` taken off again and those taken off put
 * back, leaving those changed since. */
function restorePeople(cur: string[], before: string[], after: string[]): string[] {
  const kept = cur.filter((u) => !after.includes(u) || before.includes(u));
  return [...new Set([...kept, ...before.filter((u) => !after.includes(u))])].sort();
}

/** `n` keys in order after everything in `keys`, for things new or moved to the end. */
export function lastKeys(keys: string[], n: number): string[] {
  return generateNKeysBetween(keys.reduce<string | null>((max, k) => (max === null || cmp(k, max) > 0 ? k : max), null), null, n);
}

/** A key after everything in `keys`, for something new at the end. */
export const lastKey = (keys: string[]): string => lastKeys(keys, 1)[0];

/** Whether a change that took how often a task was done from `before` to `after` is still the last
 * that counts, as `later`, how changes since took it, have it: each since took back the one before it
 * that still counted, or counted itself (docs/design.md#改动记录和撤销). As `still_last` in
 * crates/server/src/mutate/undo.rs; both are checked against fixtures/undo-done.json. */
export function stillLast<T>(before: T, after: T, later: [T, T][]): boolean {
  const changes: [T, T][] = [[before, after], ...later];
  const counting: number[] = [];
  changes.forEach(([b, a], i) => {
    const top = counting.at(-1);
    if (top !== undefined && same(changes[top], [a, b])) counting.pop();
    else counting.push(i);
  });
  return counting.length === 1 && counting[0] === 0;
}

/** `cur` with the fields that changed from `before` to `after` set back to `before`, so that undoing
 * a change leaves later changes to other fields alone. */
function restore<T extends object>(cur: T, before: T, after: T, fields: readonly (keyof T)[]): T {
  const restored = { ...cur };
  for (const f of fields) if (!same(before[f], after[f])) restored[f] = before[f];
  return restored;
}

/** `cur`'s due date with what changed from `before` to `after` set back, part by part: the date, the
 * time with the reminders, and the repeat. Undoing moving a repeating task on so keeps reminders or a
 * repeat changed since, or cleared since with the due date; without a date to go on, the other parts
 * have nothing to come back to. */
function restoreDue(cur: Due | null, before: Due | null, after: Due | null): Due | null {
  if (!before || !after) return same(before, after) ? cur : before;
  const date = before.date !== after.date ? before.date : cur?.date;
  if (!date) return null;
  const at = before.time !== after.time || !same(before.reminders, after.reminders) ? before : cur;
  const repeat = same(before.repeat, after.repeat) ? cur?.repeat : before.repeat;
  return {
    date,
    ...(at?.time && { time: at.time }),
    ...(at?.reminders && { reminders: at.reminders }),
    ...(repeat && { repeat }),
  };
}

/** A due date it can repeat from: not after the last day it repeats on, and in the lunar calendar's
 * table for a repeat by it. */
function checkRepeat(title: string, due: Due | null) {
  if (due?.repeat?.until && due.repeat.until < due.date) throw new Rejected("invalid", `“${title}”的重复到不能早于截止日期`);
  if (due?.repeat?.lunar && !lunarDate(due.date)) throw new Rejected("invalid", `“${title}”的按农历重复的截止日期应在 1900-01-31 到 2101-01-28 之间`);
}

const bothDue = () => new Rejected("invalid", "due 和 due_after 只能给一个：截止日期，或者前置完成后几天内");

/** `t` with the due date its `due_after` gives it, when it has that: none while its prerequisites are
 * not all done, then counting from the day they were, or its start if later (docs/design.md#前置完成后的截止). */
function dueAfter(t: Task): Task {
  if (!t.due_after) return t;
  const on = t.unlocked_on;
  if (!on) return { ...t, due: null };
  const { days, ...timing } = t.due_after;
  return { ...t, due: { date: plus(t.start && t.start > on ? t.start : on, days), ...timing } };
}

/** Reminders by a start at `time` as given, as the server keeps them. */
function byStart(time: string | undefined, given: Remind[]): Remind[] | undefined {
  if (time === undefined && given.some((r) => "before" in r)) throw new Rejected("invalid", "提前多久的提醒需要具体的开始时间");
  return listed(startInOrder(given));
}

/** `t`'s reminders by the start that still have something to go by: none without a start, those so long
 * before only with its time (docs/design.md#提醒). */
const fitStart = (t: Task) => listed((t.start_reminders ?? []).filter((r) => t.start && (t.start_time || "days" in r)));

/** A list as the server has it: none when empty. */
const listed = <T>(l: T[]) => (l.length ? l : undefined);

/** What the server rejects too, so that a change it would refuse isn't applied here meanwhile. */
function checkTask(t: Task) {
  if (t.done_at !== null && t.due?.repeat) throw new Rejected("invalid", `“${t.title}”已${t.dropped ? "放弃" : "完成"}，不能设置重复，请先重新打开`);
  checkRepeat(t.title, t.due);
  const reminders = ((t.due_after ?? t.due)?.reminders?.length ?? 0) + (t.start_reminders?.length ?? 0) + (t.remind_at?.length ?? 0);
  if (reminders > MAX_REMINDERS) throw new Rejected("invalid", `“${t.title}”的提醒最多 ${MAX_REMINDERS} 个`);
  if (!t.start) {
    if (t.start_time) throw new Rejected("invalid", "设置开始时刻须先有开始日期");
    if (t.start_reminders) throw new Rejected("invalid", "按开始时间算的提醒需要开始时间");
    return;
  }
  // Due the day they are done, it may come to be due the day it starts (docs/design.md#前置完成后的截止).
  if (t.due_after?.days === 0 && t.due_after.time && t.start_time && t.start_time > t.due_after.time) {
    throw new Rejected("invalid", `“${t.title}”的开始时间不能晚于截止时间`);
  }
  if (t.due && t.start > t.due.date) throw new Rejected("invalid", `“${t.title}”的开始日期不能晚于截止日期`);
  if (t.due?.time && t.start_time && t.start === t.due.date && t.start_time > t.due.time) {
    throw new Rejected("invalid", `“${t.title}”的开始时间不能晚于截止时间`);
  }
}

/** Equal as data: entities and their fields are plain JSON. */
function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
