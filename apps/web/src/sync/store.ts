// The local copy of a user's data. See `docs/web.md#数据`: the base is what the server confirmed,
// the outbox holds changes made here that it has not confirmed yet, and the view is the base with
// the outbox applied, which is what the app shows.

import { v7 as uuidv7 } from "uuid";
import type { Effect } from "../api/types/Effect";
import type { ErrorBody } from "../api/types/ErrorBody";
import type { LogEntry } from "../api/types/LogEntry";
import type { Mutation } from "../api/types/Mutation";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { SeqSnapshot } from "../api/types/SeqSnapshot";
import type { Snapshot } from "../api/types/Snapshot";
import type { TaskId } from "../api/types/TaskId";
import { type Data, Rejected, apply, copy, fromSnapshot, replay, toSnapshot } from "../domain/apply";
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
   * another tab. Resolves once written, or failed to be. */
  save(copy: Copy, queued: Pending[], settled: MutationId[]): Promise<void>;
  send(m: Mutation): void;
  /** Whether the files of the attachments `m` adds are on the server. */
  uploaded(m: Mutation): boolean;
  /** A change waits for its files to be uploaded; or, `missing`, the server has not got them after
   * all, to be uploaded again, `uploaded` saying no from now on until they are (docs/web.md#附件). */
  awaiting(m: Mutation, missing: boolean): void;
  rejected(m: Mutation, e: ErrorBody): void;
  /** Changes that arrived from elsewhere: other devices, or agents with a token. */
  arrived(entries: LogEntry[]): void;
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
    return { seq: 0, base: { folders: [], tasks: [], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] }, outbox: [], log: [] };
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getView = () => this.view;

  /** Moves the view on as a task's start comes, what starts then can be done, or back as the clock is
   * turned back past one; and on a new day (docs/design.md#开始时间). */
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
    const pending = { mutation: { id: after(this.outbox), ops }, at: Date.now() };
    const effects = apply(copy(this.data), ops, this.context(pending.at));
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

  /** Takes in changes made on this device without the page, by the Android widget and reminders, that
   * the server may not have yet: they join the outbox in the order made, as if made here, and are
   * sent once saved (docs/android.md#原生发出的改动). One the copy has already is left out; one sent
   * twice the server takes once. */
  adopt(mutations: Mutation[]) {
    const fresh = mutations
      .filter((m) => !this.log.has(m.id) && !this.outbox.some((p) => p.mutation.id === m.id))
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
   * takes back the last time a repeating task was done (docs/ui.md#弹窗对话框提示条). Only a change of
   * that task alone: undoing one of several, done together, would take back what was done to the
   * others since, too. */
  completedAt(task: TaskId, count: number): MutationId | undefined {
    const changes = [...this.log.keys(), ...this.outbox.map((p) => p.mutation.id)].reverse();
    // A change stuck in the outbox has no effects here yet.
    const id = changes.find((id) =>
      this.effectsOf(id)?.some((e) => e.kind === "task" && e.before && e.after?.id === task && e.after.completions?.count === count && (e.before.completions?.count ?? 0) === count - 1),
    );
    // Nor one that made it too: undoing that deletes it.
    return id && this.effectsOf(id)!.every((e) => e.kind === "task" && e.before && e.after?.id === task) ? id : undefined;
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
    this.outbox = this.outbox.filter((p) => !arrived.has(p.mutation.id));
    this.changed([], [...arrived]);
    const others = entries.filter((e) => !mine.has(e.id));
    if (others.length) this.hooks.arrived(others);
    this.flush();
  }

  /** The server has a change, as of `seq`. Usually the change itself arrives next as `changes`,
   * which moves it into the base. But a base that already includes `seq` (downloaded afresh, or
   * merged from another tab's copy) has it already, and it won't arrive again: drop it here. */
  acked(id: MutationId, seq: number) {
    if (seq > this.seq || !this.outbox.some((p) => p.mutation.id === id)) return;
    this.outbox = this.outbox.filter((p) => p.mutation.id !== id);
    this.changed([], [id]);
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
    this.changed([], [id]);
    this.hooks.rejected(pending.mutation, error);
    this.flush();
  }

  /** Another tab heard the server turn a change down, or this device withdrew it: drop it here too,
   * without telling again. Resolves once that is saved. */
  drop(id: MutationId): Promise<void> {
    if (!this.outbox.some((p) => p.mutation.id === id)) return Promise.resolve();
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
    this.changed();
  }

  private context(now: number) {
    return { me: this.me, now, effectsOf: this.effectsOf };
  }

  private remember(id: MutationId, effects: Effect[]) {
    this.log.delete(id);
    this.log.set(id, effects);
    if (this.log.size > LOG_SIZE) this.log.delete(this.log.keys().next().value!);
  }

  private changed(queued: Pending[] = [], settled: MutationId[] = []): Promise<void> {
    this.recompute();
    const saved = this.hooks.save({ seq: this.seq, base: toSnapshot(this.base), log: [...this.log] }, queued, settled);
    for (const listener of this.listeners) listener();
    return saved;
  }

  private recompute() {
    const d = copy(this.base);
    const stuck = new Set<MutationId>();
    this.local = new Map();
    for (const p of this.outbox) {
      try {
        this.local.set(p.mutation.id, apply(d, p.mutation.ops, this.context(p.at)));
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
