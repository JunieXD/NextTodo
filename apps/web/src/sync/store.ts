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
import { type Data, Rejected, apply, copy, fromSnapshot, replay, toSnapshot } from "../domain/apply";
import { Board } from "../domain/board";

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
  private view!: View;
  private data!: Data;
  private readonly listeners = new Set<() => void>();

  constructor(
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
    return { seq: 0, base: { folders: [], tasks: [], dependencies: [] }, outbox: [], log: [] };
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getView = () => this.view;

  /** The sequence number of the base: everything up to it is in. */
  get since() {
    return this.seq;
  }

  /** Changes saved and still waiting for the server, in the order they were made. */
  get unconfirmed(): Mutation[] {
    return this.outbox.filter((p) => !this.saving.has(p.mutation.id)).map((p) => p.mutation);
  }

  /** Makes a change here at once and queues it for the server. Throws `Rejected` when it cannot
   * apply, in which case nothing is queued. */
  commit(ops: Op[]): { id: MutationId; effects: Effect[] } {
    const pending = { mutation: { id: uuidv7(), ops }, at: Date.now() };
    const effects = apply(copy(this.data), ops, this.context(pending.at));
    this.outbox.push(pending);
    // Sent once saved: a tab that hears of it from the server takes it out of the saved outbox,
    // which only works if it is there by then.
    const { id } = pending.mutation;
    this.saving.add(id);
    void this.changed([pending]).then(() => {
      this.saving.delete(id);
      if (this.outbox.includes(pending)) this.hooks.send(pending.mutation);
    });
    return { id, effects };
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
  }

  /** The server has a change, as of `seq`. Usually the change itself arrives next as `changes`,
   * which moves it into the base. But a base that already includes `seq` (downloaded afresh, or
   * merged from another tab's copy) has it already, and it won't arrive again: drop it here. */
  acked(id: MutationId, seq: number) {
    if (seq > this.seq || !this.outbox.some((p) => p.mutation.id === id)) return;
    this.outbox = this.outbox.filter((p) => p.mutation.id !== id);
    this.changed([], [id]);
  }

  /** The server turned a change down: drop it, which takes its effect out of the view. */
  reject(id: MutationId, error: ErrorBody) {
    const pending = this.outbox.find((p) => p.mutation.id === id);
    if (!pending) return;
    this.outbox = this.outbox.filter((p) => p !== pending);
    this.changed([], [id]);
    this.hooks.rejected(pending.mutation, error);
  }

  /** Another tab heard the server turn a change down: drop it here too, without telling again. */
  drop(id: MutationId) {
    if (!this.outbox.some((p) => p.mutation.id === id)) return;
    this.outbox = this.outbox.filter((p) => p.mutation.id !== id);
    this.changed([], [id]);
  }

  /** A fresh copy of everything from the server; changes not yet confirmed stay queued. */
  resync(snapshot: SeqSnapshot) {
    const fresh = fromSnapshot(snapshot);
    this.base.folders = fresh.folders;
    this.base.tasks = fresh.tasks;
    this.base.deps = fresh.deps;
    this.seq = snapshot.seq;
    this.log.clear();
    this.changed();
  }

  private context(now: number) {
    return { now, effectsOf: this.effectsOf };
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
    this.view = { board: new Board(toSnapshot(d)), stuck, pending: this.outbox.length };
  }
}
