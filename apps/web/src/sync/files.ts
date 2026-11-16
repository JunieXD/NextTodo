// Attachments' files on this device (docs/web.md#附件): those added here, kept until the server has
// them and after, and those downloaded to be shown, kept for showing again, offline too. In
// IndexedDB's `files`, by [user id, attachment id].
//
// One tab at a time uploads, the one holding the lock: it goes through the outbox all tabs share, a
// file at a time, and tells the others of each it uploads, so that the change waiting for it goes.

import { ApiError, fileRequest } from "../api/http";
import type { Attachment } from "../api/types/Attachment";
import type { Mutation } from "../api/types/Mutation";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import { allStaged, withdraw, withdrawn, withdrawnGone } from "./drafts";
import { all, db, outbox, unqueue } from "./persist";
import type { Store } from "./store";

/** A file as kept. */
interface Kept {
  blob: Blob;
  /** Whether the server has it. */
  uploaded: boolean;
  /** When it was last shown, for clearing the longest unseen first. */
  shown: number;
}

/** How much downloaded files may take on the device, and what clearing brings it down to. */
const MAX_BYTES = 500 * 1024 * 1024;
const KEEP_BYTES = 400 * 1024 * 1024;

const FIRST_RETRY = 1000;
const LAST_RETRY = 30_000;

type Add = Extract<Op, { op: "attachment.add" }>;
const adds = (m: Mutation): Add[] => m.ops.filter((op): op is Add => op.op === "attachment.add");

/** Deletes files kept for an add box that were never added. */
export async function dropFiles(userId: string, ids: string[]) {
  const tx = (await db()).transaction("files", "readwrite");
  await Promise.all([...ids.map((id) => tx.store.delete([userId, id])), tx.done]);
}

/** What a file is shown by. */
type Shown = Pick<Attachment, "id" | "type">;

/** What the uploading tab tells the others. */
export type FileMessage = { uploaded: string };

export interface FileHooks {
  /** Tells the other tabs. */
  broadcast(m: FileMessage | { rejected: MutationId }): void;
  /** Tells the user. */
  told(text: string): void;
  /** The server no longer accepts the session. */
  signedOut(): void;
}

/** A session's files: what is uploaded, for the outbox to know what can go; uploading; and fetching
 * files to show. */
export class Files {
  private readonly uploaded = new Set<string>();
  /** Files the server turned out to be missing, until marked so where kept: what is kept says they
   * are uploaded till then. */
  private readonly lost = new Set<string>();
  /** Waiting for the lock, so not asking for it again. */
  private requested = false;
  private retry = FIRST_RETRY;
  private timer: ReturnType<typeof setTimeout> | undefined;
  /** Aborted as the session stops: what is in flight ends, and nothing more is written, the user's data
   * being about to be deleted. */
  private readonly stopping = new AbortController();
  private readonly fetching = new Map<string, Promise<Blob>>();
  /** Shown in this session: noted once. */
  private readonly seen = new Set<string>();

  constructor(
    private readonly user: string,
    private readonly store: Store,
    private readonly hooks: FileHooks,
  ) {
    addEventListener("online", this.wake);
    document.addEventListener("visibilitychange", this.wake);
    void this.load();
  }

  /** Whether the session has ended: nothing more is written for it. */
  get stopped() {
    return this.stopping.signal.aborted;
  }

  stop() {
    this.stopping.abort();
    clearTimeout(this.timer);
    removeEventListener("online", this.wake);
    document.removeEventListener("visibilitychange", this.wake);
  }

  /** Learns which kept files the server has. */
  private async load() {
    const tx = (await db()).transaction("files");
    for await (const c of tx.store.iterate(all(this.user))) if ((c.value as Kept).uploaded) this.uploaded.add((c.key as string[])[1]);
    this.store.flush();
  }

  /** Keeps `blob` as attachment `id`'s file, to upload once added; nothing once the session has ended,
   * the user's data being about to be deleted. */
  async keep(id: string, blob: Blob) {
    const d = await db();
    // As in `update`: ending meanwhile, the file is deleted with the rest.
    if (this.stopped) return;
    await d.put("files", { blob, uploaded: false, shown: Date.now() } satisfies Kept, [this.user, id]);
  }

  /** Changes file `id`'s record as it is when written: other tabs change it too. */
  private async update(id: string, change: Partial<Kept>) {
    const d = await db();
    // A transaction made from here on comes before the deleting that follows the session's end.
    if (this.stopped) return;
    const tx = d.transaction("files", "readwrite");
    const kept: Kept | undefined = await tx.store.get([this.user, id]);
    if (kept) await tx.store.put({ ...kept, ...change }, [this.user, id]);
    await tx.done;
  }

  /** Whether the files of the attachments `m` adds are on the server. */
  has = (m: Mutation) => adds(m).every((a) => this.uploaded.has(a.id));

  /** `m` waits for its files: uploads them; again when the server is `missing` them, which until then
   * holds `m` back, from now on. */
  awaiting = (m: Mutation, missing: boolean) => {
    if (!missing) return this.upload();
    for (const a of adds(m)) {
      this.uploaded.delete(a.id);
      this.lost.add(a.id);
    }
    void this.missing(adds(m).map((a) => a.id));
  };

  /** Another tab uploaded a file. */
  heard(m: FileMessage) {
    this.uploaded.add(m.uploaded);
    this.store.flush();
  }

  private async missing(ids: string[]) {
    for (const id of ids) {
      await this.update(id, { uploaded: false });
      this.lost.delete(id);
    }
    this.upload();
  }

  private wake = () => {
    if (document.visibilityState !== "visible" || !this.timer) return;
    clearTimeout(this.timer);
    this.timer = undefined;
    this.upload();
  };

  /** Asks for the lock, unless asked already; holding it, uploads what the outbox waits for. */
  private upload() {
    if (this.requested || this.timer || this.stopped) return;
    this.requested = true;
    void navigator.locks.request(`upload:${this.user}`, async () => {
      // From here, work joining the outbox asks again.
      this.requested = false;
      while (!this.stopped) {
        const next = await this.next();
        if (!next) return;
        const [m, add, kept] = next;
        try {
          await fileRequest("PUT", add.id, this.stopping.signal, { blob: kept.blob, type: add.type });
        } catch (e) {
          if (!(e instanceof ApiError)) throw e;
          if (this.stopped) return;
          if (e.status === 401) return this.hooks.signedOut();
          // Not reached, or a failure that may pass.
          if (![400, 403, 409, 413].includes(e.status)) return this.later();
          await this.refused(m, add, e);
          continue;
        }
        await this.update(add.id, { uploaded: true });
        if (this.stopped) return;
        this.uploaded.add(add.id);
        this.hooks.broadcast({ uploaded: add.id });
        this.retry = FIRST_RETRY;
        this.store.flush();
      }
    });
  }

  /** Tries again a while later, sooner when the page comes back or the device goes online. */
  private later() {
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.upload();
    }, this.retry);
    this.retry = Math.min(this.retry * 2, LAST_RETRY);
  }

  /** The first file the outbox waits for, with the change it is for. A tab that closed halfway may
   * have left changes withdrawn into a draft in the outbox or in the other tabs: they go first, from
   * everywhere. It may also have uploaded a file without telling: this tab learns it here. */
  private async next(): Promise<[Mutation, Add, Kept] | undefined> {
    for (const id of withdrawn(this.user)) await this.unqueue(id);
    for (const { mutation: m } of await outbox(this.user)) {
      for (const add of adds(m)) {
        const kept: Kept | undefined = await (await db()).get("files", [this.user, add.id]);
        if (!kept) continue;
        if (!kept.uploaded) return [m, add, kept];
        if (!this.uploaded.has(add.id) && !this.lost.has(add.id)) {
          this.uploaded.add(add.id);
          this.store.flush();
        }
      }
    }
    return undefined;
  }

  /** The server turned the file down for good: the change goes, as one it turned down; an AI job's
   * text and files back to the add box it came from (docs/web.md#附件). Noted as withdrawn first, so
   * that a tab closing halfway leaves the rest to the next one uploading. */
  private async refused(m: Mutation, add: Add, error: ApiError) {
    const reason = refusal(error);
    const job = m.ops.find((op) => op.op === "ai_job.create");
    withdraw(
      this.user,
      m.id,
      job && {
        place: job.folder_id ?? "root",
        text: job.text,
        files: adds(m).map((a) => ({ id: a.id, name: a.name, type: a.type, size: a.size, ...(a.id === add.id && { error: reason }) })),
      },
    );
    await this.unqueue(m.id);
    this.hooks.told(`“${add.name}”上传失败：${reason}${job ? "，AI 整理的内容已放回输入框" : ""}`);
  }

  /** Takes change `id`, noted as withdrawn, out of the outbox, here and in the other tabs, as one the
   * server turned down; the note goes once the other tabs are told. */
  private async unqueue(id: MutationId) {
    await unqueue(this.user, id);
    this.hooks.broadcast({ rejected: id });
    withdrawnGone(this.user, [id]);
    await this.store.drop(id);
  }

  /** Attachment `a`'s file, or a file staged to be one: kept here, or downloaded and kept. */
  blob(a: Shown): Promise<Blob> {
    let found = this.fetching.get(a.id);
    if (!found) {
      found = this.fetch(a).finally(() => this.fetching.delete(a.id));
      this.fetching.set(a.id, found);
    }
    return found;
  }

  private async fetch(a: Shown): Promise<Blob> {
    const key = [this.user, a.id];
    const kept: Kept | undefined = await (await db()).get("files", key);
    if (kept) {
      if (!this.seen.has(a.id)) {
        this.seen.add(a.id);
        await this.update(a.id, { shown: Date.now() });
      }
      return kept.blob;
    }
    const blob = new Blob([await fileRequest("GET", a.id, this.stopping.signal)], { type: a.type });
    this.seen.add(a.id);
    const d = await db();
    if (this.stopped) return blob;
    await d.put("files", { blob, uploaded: true, shown: Date.now() } satisfies Kept, key);
    void this.clear();
    return blob;
  }

  /** Brings the files kept down to `KEEP_BYTES` once they pass `MAX_BYTES`, the longest unseen
   * first; not those the outbox or an add box still has to upload. */
  private async clear() {
    const kept: { id: string; size: number; shown: number; uploaded: boolean }[] = [];
    const tx = (await db()).transaction("files");
    for await (const c of tx.store.iterate(all(this.user))) {
      const v = c.value as Kept;
      kept.push({ id: (c.key as string[])[1], size: v.blob.size, shown: v.shown, uploaded: v.uploaded });
    }
    let total = kept.reduce((n, f) => n + f.size, 0);
    if (total <= MAX_BYTES) return;
    const needed = new Set([...(await outbox(this.user)).flatMap((p) => adds(p.mutation).map((a) => a.id)), ...allStaged(this.user).map((s) => s.id)]);
    const write = (await db()).transaction("files", "readwrite");
    for (const f of kept.filter((f) => f.uploaded && !needed.has(f.id)).sort((a, b) => a.shown - b.shown)) {
      if (total <= KEEP_BYTES) break;
      await write.store.delete([this.user, f.id]);
      total -= f.size;
    }
    await write.done;
  }
}

/** What to say of a file turned down: what a proxy in front of the server says of a large one is
 * not the server's words. */
const refusal = (e: ApiError) => (e.status === 413 ? "文件超过服务器允许的大小" : e.message);
