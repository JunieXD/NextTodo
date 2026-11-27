// The local copy of a user's data. See `docs/web.md#数据`: the base is what the server confirmed,
// the outbox holds changes made here that it has not confirmed yet, and the view is the base with
// the outbox applied, which is what the app shows.

import { v7 as uuidv7 } from "uuid";
import type { AttachmentId } from "../api/types/AttachmentId";
import type { Effect } from "../api/types/Effect";
import type { ErrorBody } from "../api/types/ErrorBody";
import type { LogEntry } from "../api/types/LogEntry";
import type { Mutation } from "../api/types/Mutation";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { SeqSnapshot } from "../api/types/SeqSnapshot";
import type { Snapshot } from "../api/types/Snapshot";
import type { TaskId } from "../api/types/TaskId";
import { type Data, Rejected, apply, copy, fromSnapshot, localOf, replay, toSnapshot } from "../domain/apply";
import { Board, before, cmp, nowOf } from "../domain/board";

/** A change made here, with when it was made, so that applying it again gives the same times. */
export interface Pending {
  mutation: Mutation;
  at: number;
}

/** What is kept between visits. */
export interface Saved {
  seq: number;
  base: Snapshot;
  outbox: Pending[];
  /** Recent changes by mutation, oldest first, for undoing them here before the server has. */
  log: [MutationId, Effect[]][];
}

/** What the server confirmed, as this copy has it: what is saved but the outbox, which is saved by
 * the changes that join and leave it. */
export type Copy = Omit<Saved, "outbox">;

export interface View {
  board: Board;
  /** Changes in the outbox that cannot be applied here; they wait for the server's verdict. */
  stuck: Set<MutationId>;
  /** How many changes the server has not confirmed yet. */
  pending: number;
}

export interface Hooks {
  /** Saves the copy, and the changes that `queued` joined the outbox and `settled` left it: those
   * the server confirmed or turned down. Every change that arrives settles, also one queued in
   * another tab. Resolves once written, true, or failed to be. */
  save(copy: Copy, queued: Pending[], settled: MutationId[]): Promise<boolean>;
  send(m: Mutation): void;
  /** Whether the files of the attachments `m` adds are on the server. */
  uploaded(m: Mutation): boolean;
  /** A change waits for its files to be uploaded; or, `missing`, the server has not got them after
   * all, to be uploaded again, `uploaded` saying no from now on until they are (docs/web.md#附件). */
  awaiting(m: Mutation, missing: boolean): void;
  rejected(m: Mutation, e: ErrorBody): void;
  /** Changes that arrived from elsewhere: other devices, or agents with a token. */
  arrived(entries: LogEntry[]): void;
  /** Changes of the outbox the server confirmed, once they have left the saved outbox too. */
  confirmed(ms: Mutation[]): void;
}

/** How many recent changes are kept for undo. */
const LOG_SIZE = 200;

export class Store {
  private readonly base: Data;
  private seq: number;
  private outbox: Pending[];
  private readonly log: Map<MutationId, Effect[]>;
  /** Effects of the outbox changes, as last applied. */
  private local = new Map<MutationId, Effect[]>();
  /** Changes made here that are not saved yet, and so not sent. */
  private readonly saving = new Set<MutationId>();
  /** Changes handed to `send` since the connection last caught up. */
  private released = new Set<MutationId>();
  /** The last save asked for: saves are written in turn, so this one is the last to be. */
  private lastSave = Promise.resolve(true);
  /** Changes made here that the server turned down, or this device withdrew. */
  private readonly turnedDown = new Set<MutationId>();
  private view!: View;
  private data!: Data;
  private readonly listeners = new Set<() => void>();

  constructor(
    /** Whose data this is. */
    private readonly me: string,
    saved: Saved,
    private readonly hooks: Hooks,
  ) {
    this.base = fromSnapshot(saved.base);
    this.seq = saved.seq;
    this.outbox = saved.outbox;
    this.log = new Map(saved.log);
    this.recompute();
  }

  static empty(): Saved {
    return { seq: 0, base: { folders: [], tasks: [], dependencies: [], completions: [], attachments: [], ai_jobs: [], shares: [], people: [] }, outbox: [], log: [] };
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getView = () => this.view;

  /** Whether the changes made so far are written on this device, once they are or writing them failed. */
  saved = () => this.lastSave;

  /** Where change `id`, made here, stands with the server. */
  standing(id: MutationId): "waiting" | "confirmed" | "turned down" {
    if (this.outbox.some((p) => p.mutation.id === id)) return "waiting";
    return this.turnedDown.has(id) ? "turned down" : "confirmed";
  }

  /** Moves the view on as a task's start comes, what starts then can be done, or back as the clock is
   * turned back past one; and on a new day (docs/design/dates.md#开始时间). */
  tick = () => {
    const now = nowOf(new Date());
    const { board } = this.view;
    // By the user's day and time, not the moment: a start comes or goes again also as the clock or the
    // time zone is turned back.
    const crossed = board.snapshot.tasks.some(
      (t) => t.done_at === null && t.start !== null && before(board.now, t.start, t.start_time) !== before(now, t.start, t.start_time),
    );
    if (!crossed && now.today === board.now.today) return;
    this.recompute();
    for (const listener of this.listeners) listener();
  };

  /** The sequence number of the base: everything up to it is in. */
  get since() {
    return this.seq;
  }

  /** Changes saved and still waiting for the server that can go now, in the order they were made.
   * One adding attachments goes once their files are uploaded, and those after it once the server has
   * it: should its files be missing there, it goes again, still before them. */
  get unconfirmed(): Mutation[] {
    return this.sendable().ready;
  }

  /** What can go now, and the change waiting for its files, if any. */
  private sendable(): { ready: Mutation[]; waiting?: Mutation } {
    const ready: Mutation[] = [];
    for (const { mutation: m } of this.outbox) {
      if (this.saving.has(m.id)) continue;
      if (!m.ops.some((op) => op.op === "attachment.add")) {
        ready.push(m);
        continue;
      }
      if (!this.hooks.uploaded(m)) return { ready, waiting: m };
      ready.push(m);
      break;
    }
    return { ready };
  }

  /** The connection has caught up: everything still unconfirmed that can go goes again. */
  connected() {
    this.released = new Set();
    this.flush();
  }

  /** Sends what can go and has not been sent on this connection: after a change is saved, its files
   * uploaded, or one it waited for confirmed. */
  flush = () => {
    const { ready, waiting } = this.sendable();
    for (const m of ready) {
      if (this.released.has(m.id)) continue;
      this.released.add(m.id);
      this.hooks.send(m);
    }
    if (waiting) this.hooks.awaiting(waiting, false);
  };

  /** Makes a change here at once and queues it for the server. Throws `Rejected` when it cannot
   * apply, in which case nothing is queued. */
  commit(ops: Op[]): { id: MutationId; effects: Effect[] } {
    const now = new Date();
    const pending = { mutation: { id: after(this.outbox), ops, ...(this.countsDays(ops) && nowOf(now)) }, at: now.getTime() };
    const effects = apply(copy(this.data), ops, this.context(pending));
    this.outbox.push(pending);
    // Sent once saved: a tab that hears of it from the server takes it out of the saved outbox,
    // which only works if it is there by then.
    const { id } = pending.mutation;
    this.saving.add(id);
    void this.changed([pending]).then(() => {
      this.saving.delete(id);
      this.flush();
    });
    return { id, effects };
  }

  /** Takes in changes made on this device without the page, by the Android widget and reminders, or by
   * another tab that closed, that the server may not have yet: they join the outbox in the order made,
   * as if made here, and are sent once saved (docs/android.md#原生发出的改动). One the copy has
   * already, or knows was turned down, is left out; one sent twice the server takes once. */
  adopt(mutations: Mutation[]) {
    const fresh = mutations
      .filter((m) => !this.log.has(m.id) && !this.turnedDown.has(m.id) && !this.outbox.some((p) => p.mutation.id === m.id))
      .map((mutation) => ({ mutation, at: madeAt(mutation.id) }));
    if (!fresh.length) return;
    this.outbox = [...this.outbox, ...fresh].sort((a, b) => cmp(a.mutation.id, b.mutation.id));
    for (const p of fresh) this.saving.add(p.mutation.id);
    void this.changed(fresh).then(() => {
      for (const p of fresh) this.saving.delete(p.mutation.id);
      this.flush();
    });
  }

  /** The latest change this copy knows of that did a task one more time, up to `count`: undoing it
   * takes back the last time a repeating task was done (docs/ui/dialogs.md). Only a change of
   * that task alone: undoing one of several, done together, would take back what was done to the
   * others since, too. */
  completedAt(task: TaskId, count: number): MutationId | undefined {
    const changes = [...this.log.keys(), ...this.outbox.map((p) => p.mutation.id)].reverse();
    // A change stuck in the outbox has no effects here yet.
    const id = changes.find((id) =>
      this.effectsOf(id)?.some((e) => e.kind === "task" && e.before && e.after?.id === task && e.after.completions?.count === count && (e.before.completions?.count ?? 0) === count - 1),
    );
    // Nor one that made it too, undoing which deletes it, or changed others than it and its records.
    const its = (e: Effect) => (e.kind === "task" && e.before && e.after?.id === task) || (e.kind === "completion" && (e.before ?? e.after)!.task_id === task);
    return id && this.effectsOf(id)!.every(its) ? id : undefined;
  }

  /** The latest change this copy knows of that marked a task done. */
  doneBy(task: TaskId): MutationId | undefined {
    const changes = [...this.log.keys(), ...this.outbox.map((p) => p.mutation.id)].reverse();
    return changes.find((id) => this.effectsOf(id)?.some((e) => e.kind === "task" && e.after?.id === task && e.before?.done_at === null && e.after.done_at !== null));
  }

  /** The change still to be confirmed that made attachment `id`, adding it or bringing it back, if
   * any: the server has the attachment once it has that change. */
  madeBy(id: AttachmentId): MutationId | undefined {
    return this.outbox
      .map((p) => p.mutation.id)
      .findLast((m) => this.local.get(m)?.some((e) => e.kind === "attachment" && e.before === null && e.after?.id === id));
  }

  /** What a change did, as far as this copy knows. */
  effectsOf = (id: MutationId): Effect[] | undefined => this.log.get(id) ?? this.local.get(id);

  /** Changes from the server, in order, up to `seq`. */
  receive(entries: LogEntry[], seq: number) {
    const mine = new Set(this.outbox.map((p) => p.mutation.id));
    const arrived = new Set(entries.map((e) => e.id));
    for (const e of entries) {
      replay(this.base, e.effects);
      this.remember(e.id, e.effects);
    }
    this.seq = seq;
    const confirmed = this.outbox.filter((p) => arrived.has(p.mutation.id)).map((p) => p.mutation);
    this.outbox = this.outbox.filter((p) => !arrived.has(p.mutation.id));
    void this.changed([], [...arrived]).then(() => {
      if (confirmed.length) this.hooks.confirmed(confirmed);
    });
    const others = entries.filter((e) => !mine.has(e.id));
    if (others.length) this.hooks.arrived(others);
    this.flush();
  }

  /** The server has a change, as of `seq`. Usually the change itself arrives next as `changes`,
   * which moves it into the base. But a base that already includes `seq` (downloaded afresh, or
   * merged from another tab's copy) has it already, and it won't arrive again: drop it here. */
  acked(id: MutationId, seq: number) {
    const pending = this.outbox.find((p) => p.mutation.id === id);
    if (seq > this.seq || !pending) return;
    this.outbox = this.outbox.filter((p) => p !== pending);
    void this.changed([], [id]).then(() => this.hooks.confirmed([pending.mutation]));
    this.flush();
  }

  /** The server turned a change down: drop it, which takes its effect out of the view. One whose
   * attachments' files it is missing stays, to go again once they are uploaded again. */
  reject(id: MutationId, error: ErrorBody) {
    const pending = this.outbox.find((p) => p.mutation.id === id);
    if (!pending) return;
    if (error.code === "file_missing") {
      this.released.delete(id);
      this.hooks.awaiting(pending.mutation, true);
      return;
    }
    this.outbox = this.outbox.filter((p) => p !== pending);
    this.turnedDown.add(id);
    void this.changed([], [id]);
    this.hooks.rejected(pending.mutation, error);
    this.flush();
  }

  /** Another tab heard the server turn a change down, or this device withdrew it: drop it here too,
   * without telling again. Resolves once that is saved. */
  drop(id: MutationId): Promise<boolean> {
    // Also one not here: another tab may hand it over yet.
    this.turnedDown.add(id);
    if (!this.outbox.some((p) => p.mutation.id === id)) return Promise.resolve(true);
    this.outbox = this.outbox.filter((p) => p.mutation.id !== id);
    const saved = this.changed([], [id]);
    this.flush();
    return saved;
  }

  /** The outbox's changes, in order: those adding attachments are what the files to upload are for. */
  get queued(): Mutation[] {
    return this.outbox.map((p) => p.mutation);
  }

  /** A fresh copy of everything from the server; changes not yet confirmed stay queued. */
  resync(snapshot: SeqSnapshot) {
    Object.assign(this.base, fromSnapshot(snapshot));
    this.seq = snapshot.seq;
    this.log.clear();
    void this.changed();
  }

  private context(p: Pending) {
    return { me: this.me, now: p.at, local: localOf(p.mutation, p.at), effectsOf: this.effectsOf, effectsAfter: this.effectsAfter };
  }

  /** Whether a change is to say the day and time it is made: where a due date may count from them
   * (docs/design/dates.md#前置完成后的截止). Not otherwise, as a server older than that turns down every change
   * that does. */
  private countsDays(ops: Op[]): boolean {
    return (
      ops.some((op) => (op.op === "task.create" || op.op === "task.update") && op.due_after) ||
      [...this.data.tasks.values()].some((t) => t.due_after)
    );
  }

  /** What the changes after `id` did: those confirmed, then those made here and applied so far. */
  private effectsAfter = (id: MutationId): Effect[][] => {
    const known = [...this.log, ...this.local];
    return known.slice(known.findIndex(([k]) => k === id) + 1).map(([, effects]) => effects);
  };

  private remember(id: MutationId, effects: Effect[]) {
    this.log.delete(id);
    this.log.set(id, effects);
    if (this.log.size > LOG_SIZE) this.log.delete(this.log.keys().next().value!);
  }

  private changed(queued: Pending[] = [], settled: MutationId[] = []): Promise<boolean> {
    this.recompute();
    const saved = (this.lastSave = this.hooks.save({ seq: this.seq, base: toSnapshot(this.base), log: [...this.log] }, queued, settled));
    for (const listener of this.listeners) listener();
    return saved;
  }

  private recompute() {
    const d = copy(this.base);
    const stuck = new Set<MutationId>();
    this.local = new Map();
    for (const p of this.outbox) {
      try {
        this.local.set(p.mutation.id, apply(d, p.mutation.ops, this.context(p)));
      } catch (e) {
        if (!(e instanceof Rejected)) throw e;
        stuck.add(p.mutation.id);
      }
    }
    this.data = d;
    this.view = { board: new Board(toSnapshot(d), nowOf(new Date()), this.me), stuck, pending: this.outbox.length };
  }
}

/** When a change was made: a UUIDv7 starts with the time in milliseconds. */
const madeAt = (id: MutationId) => parseInt(id.slice(0, 8) + id.slice(9, 13), 16);

/** An id for a change made after those `queued`, which the page reads in id order when it opens
 * again: kept past the last change's time, in case the clock has gone back since, as it may have
 * when the page opened. */
function after(queued: Pending[]): MutationId {
  const last = queued.at(-1)?.mutation.id;
  return uuidv7({ msecs: Math.max(Date.now(), last ? madeAt(last) + 1 : 0) });
}
