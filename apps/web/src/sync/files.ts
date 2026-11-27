// Attachments' files on this device (docs/web.md#附件): those added here, kept until the server has
// them and after, and those downloaded to be shown, kept for showing again, offline too. In
// IndexedDB, by [user id, attachment id]: the bytes in `files`, written once, and what changes of a
// file in `fileInfo`. The bytes are never written again: WebKit loses those of a record put again
// with the blob read from it.
//
// One tab at a time uploads, the one holding the lock: it goes through the outbox all tabs share, a
// file at a time, and tells the others of each it uploads, so that the change waiting for it goes.

import { ApiError, fileRequest } from "../api/http";
import type { Attachment } from "../api/types/Attachment";
import type { Mutation } from "../api/types/Mutation";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import { allStaged, withdraw, withdrawn, withdrawnGone } from "./drafts";
import { exclusive } from "./locks";
import { all, db, outbox, unqueue } from "./persist";
import type { Store } from "./store";

/** What is known of a file kept. */
interface FileInfo {
  size: number;
  /** Whether the server has it. */
  uploaded: boolean;
  /** When it was last shown, for clearing the longest unseen first. */
  shown: number;
  /** The attachment it is a copy of, which the server copies: nothing is kept here for it. */
  from?: string;
  /** The change that made the attachment it is a copy of, when the server had not confirmed it yet:
   * the copy waits for it. */
  after?: MutationId;
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
  const tx = (await db()).transaction(["files", "fileInfo"], "readwrite");
  await Promise.all([...ids.flatMap((id) => [tx.objectStore("files").delete([userId, id]), tx.objectStore("fileInfo").delete([userId, id])]), tx.done]);
}

/** What a file is shown by. */
type Shown = Pick<Attachment, "id" | "type">;

/** What the uploading tab tells the others. */
export type FileMessage = { uploaded: string };

export interface FileHooks {
  /** Tells the other tabs. */
  broadcast(m: FileMessage | { rejected: MutationId; told?: { user: string; text: string } }): void;
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
  /** Whether the timer, while set, is only for copies waiting for changes, which a new file need not
   * wait for. */
  private held = false;
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

  /** Learns which kept files the server has; and makes room, should a tab have closed before it
   * could. */
  private async load() {
    const tx = (await db()).transaction("fileInfo");
    for await (const c of tx.store.iterate(all(this.user))) if ((c.value as FileInfo).uploaded) this.uploaded.add((c.key as string[])[1]);
    this.store.flush();
    void this.clear();
  }

  /** Keeps `blob` as attachment `id`'s file, to upload once added; nothing once the session has ended,
   * the user's data being about to be deleted. */
  keep = (id: string, blob: Blob) => this.write(id, blob, false);

  /** Notes of each `[id, from, after]` that attachment `id`'s file, to upload once added, is a copy
   * of attachment `from`'s, which the server makes once it has change `after`, if any, that made
   * `from` (docs/web.md#附件). */
  async keepCopies(copies: [string, string, MutationId | undefined][]) {
    const d = await db();
    if (this.stopped) return;
    const tx = d.transaction("fileInfo", "readwrite");
    const info = (from: string, after?: MutationId): FileInfo => ({ size: 0, uploaded: false, shown: Date.now(), from, ...(after && { after }) });
    await Promise.all([...copies.map(([id, from, after]) => tx.store.put(info(from, after), [this.user, id])), tx.done]);
  }

  private async write(id: string, blob: Blob, uploaded: boolean) {
    const d = await db();
    // As in `update`: ending meanwhile, the file is deleted with the rest.
    if (this.stopped) return;
    const tx = d.transaction(["files", "fileInfo"], "readwrite");
    const key = [this.user, id];
    const info: FileInfo = { size: blob.size, uploaded, shown: Date.now() };
    await Promise.all([tx.objectStore("files").put(blob, key), tx.objectStore("fileInfo").put(info, key), tx.done]);
  }

  /** Changes what is known of file `id` as it is when written: other tabs change it too. */
  private async update(id: string, change: Partial<FileInfo>) {
    const d = await db();
    // A transaction made from here on comes before the deleting that follows the session's end.
    if (this.stopped) return;
    const tx = d.transaction("fileInfo", "readwrite");
    const info: FileInfo | undefined = await tx.store.get([this.user, id]);
    if (info) await tx.store.put({ ...info, ...change }, [this.user, id]);
    await tx.done;
  }

  /** Whether the files of the attachments `m` adds are on the server. */
  has = (m: Mutation) => adds(m).every((a) => this.uploaded.has(a.id));

  /** `m` waits for its files: uploads them; again when the server is `missing` them, which until then
   * holds `m` back, from now on. */
  awaiting = (m: Mutation, missing: boolean) => {
    if (!missing) return this.held && this.timer ? this.retryNow() : this.upload();
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
    if (document.visibilityState === "visible") this.retryNow();
  };

  /** Tries again now rather than when the timer says. */
  private retryNow() {
    if (!this.timer) return;
    clearTimeout(this.timer);
    this.timer = undefined;
    this.upload();
  }

  /** Asks for the lock, unless asked already; holding it, uploads what the outbox waits for. */
  private upload() {
    if (this.requested || this.timer || this.stopped) return;
    this.requested = true;
    void exclusive(`upload:${this.user}`, async () => {
      // From here, work joining the outbox asks again.
      this.requested = false;
      while (!this.stopped) {
        const next = await this.next();
        if (!next || this.stopped) return;
        if (next === "later") return this.later(true);
        const [m, add, file] = next;
        if (!file) {
          await this.refused(m, add, "文件已不在这台设备上", "上传");
          continue;
        }
        try {
          await fileRequest("PUT", add.id, this.stopping.signal, typeof file === "string" ? { from: file } : { blob: file, type: add.type });
        } catch (e) {
          if (!(e instanceof ApiError)) throw e;
          if (this.stopped) return;
          if (e.status === 401) return this.hooks.signedOut();
          // Not reached, or a failure that may pass.
          if (![400, 403, 404, 409, 413].includes(e.status)) return this.later();
          // A copy the server would not make says why itself: the attachment copied is gone, say.
          const copy = typeof file === "string";
          await this.refused(m, add, copy ? e.message : refusal(e), copy ? "复制" : "上传");
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
  private later(held = false) {
    this.held = held;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.upload();
    }, this.retry);
    this.retry = Math.min(this.retry * 2, LAST_RETRY);
  }

  /** The first file the outbox waits for, with the change it is for: the file, or the attachment it
   * is a copy of. A copy of one the server has not yet, its change still to be confirmed, is passed
   * over, and `later` said when nothing else is waiting: the change may be a tab's that closed before
   * sending it, to go when the app opens again, and files after it go meanwhile. A tab that closed
   * halfway may have left changes withdrawn into a draft in the outbox or in the other tabs: they go
   * first, from everywhere. It may also have uploaded a file without telling: this tab learns it
   * here. No file means it is gone from the device, as when the add box it was in dropped it
   * meanwhile. */
  private async next(): Promise<[Mutation, Add, Blob | string | undefined] | "later" | undefined> {
    for (const id of withdrawn(this.user)) await this.unqueue(id);
    const queued = await outbox(this.user);
    let held = false;
    for (const { mutation: m } of queued) {
      for (const add of adds(m)) {
        // Read together, with whether the change is still waiting: meanwhile an add box in another tab
        // may drop the file, or another tab hear the change confirmed and clear its file.
        const tx = (await db()).transaction(["outbox", "files", "fileInfo"]);
        const key = [this.user, add.id];
        const [waiting, info, blob]: [IDBValidKey | undefined, FileInfo | undefined, Blob | undefined] = await Promise.all([
          tx.objectStore("outbox").getKey([this.user, m.id]),
          tx.objectStore("fileInfo").get(key),
          tx.objectStore("files").get(key),
        ]);
        if (!waiting) break;
        if (info?.after && !info.uploaded && queued.some((p) => p.mutation.id === info.after)) {
          held = true;
          continue;
        }
        if (!info?.uploaded) return [m, add, blob ?? info?.from];
        if (!this.uploaded.has(add.id) && !this.lost.has(add.id)) {
          this.uploaded.add(add.id);
          this.store.flush();
        }
      }
    }
    return held ? "later" : undefined;
  }

  /** The file cannot go, for `reason`: the change goes, as one the server turned down; an AI job's
   * text and files back to the add box it came from, else its files from the device
   * (docs/web.md#附件). Noted as withdrawn first, so that a tab closing halfway leaves the rest to the
   * next one uploading. */
  private async refused(m: Mutation, add: Add, reason: string, failed: "上传" | "复制") {
    const job = m.ops.find((op) => op.op === "ai_job.create");
    // First: a tab closing right after leaves no file behind, and the change, its file gone, goes the
    // next time.
    if (!job) await dropFiles(this.user, adds(m).map((a) => a.id));
    withdraw(
      this.user,
      m.id,
      job && {
        place: job.day ? "calendar" : (job.folder_id ?? "root"),
        text: job.text,
        files: adds(m).map((a) => ({ id: a.id, name: a.name, type: a.type, size: a.size, ...(a.id === add.id && { error: reason }) })),
      },
    );
    const told = `“${add.name}”${failed}失败：${reason}${job ? "，AI 整理的内容已放回输入框" : ""}`;
    await this.unqueue(m.id, told);
    this.hooks.told(told);
  }

  /** Takes change `id`, noted as withdrawn, out of the outbox, here and in the other tabs, as one the
   * server turned down, with why to tell, should another be where the user is; the note goes once the
   * other tabs are told. */
  private async unqueue(id: MutationId, told?: string) {
    await unqueue(this.user, id);
    this.hooks.broadcast({ rejected: id, ...(told && { told: { user: this.user, text: told } }) });
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
    const d = await db();
    const kept: Blob | undefined = await d.get("files", [this.user, a.id]);
    if (kept) {
      if (!this.seen.has(a.id)) {
        this.seen.add(a.id);
        await this.update(a.id, { shown: Date.now() });
      }
      return kept;
    }
    // A copy the server has yet to make shows the file it copies.
    const info: FileInfo | undefined = await d.get("fileInfo", [this.user, a.id]);
    if (info?.from && !info.uploaded) return this.blob({ id: info.from, type: a.type });
    const blob = new Blob([await fileRequest("GET", a.id, this.stopping.signal)], { type: a.type });
    this.seen.add(a.id);
    await this.write(a.id, blob, true);
    void this.clear();
    return blob;
  }

  /** Changes the server confirmed left the outbox: the files of those adding attachments may go now
   * to make room, and what was noted of copies, which keep nothing here, goes. */
  confirmed(ms: Mutation[]) {
    const added = ms.flatMap((m) => adds(m).map((a) => a.id));
    if (added.length) void this.forgetCopies(added).then(() => this.clear());
    // A copy waiting for them goes now.
    this.retryNow();
  }

  private async forgetCopies(ids: string[]) {
    const d = await db();
    if (this.stopped) return;
    const tx = d.transaction("fileInfo", "readwrite");
    for (const id of ids) if (((await tx.store.get([this.user, id])) as FileInfo | undefined)?.from) await tx.store.delete([this.user, id]);
    await tx.done;
  }

  /** Brings the files kept down to `KEEP_BYTES` once they pass `MAX_BYTES`, the longest unseen
   * first; not those the outbox or an add box still needs: until confirmed, the server may turn out
   * to be missing one. */
  private async clear() {
    const kept: (FileInfo & { id: string })[] = [];
    const tx = (await db()).transaction("fileInfo");
    for await (const c of tx.store.iterate(all(this.user))) kept.push({ ...(c.value as FileInfo), id: (c.key as string[])[1] });
    let total = kept.reduce((n, f) => n + f.size, 0);
    if (total <= MAX_BYTES) return;
    const needed = new Set([...(await outbox(this.user)).flatMap((p) => adds(p.mutation).map((a) => a.id)), ...allStaged(this.user).map((s) => s.id)]);
    const write = (await db()).transaction(["files", "fileInfo"], "readwrite");
    for (const f of kept.filter((f) => f.uploaded && !needed.has(f.id)).sort((a, b) => a.shown - b.shown)) {
      if (total <= KEEP_BYTES) break;
      await Promise.all([write.objectStore("files").delete([this.user, f.id]), write.objectStore("fileInfo").delete([this.user, f.id])]);
      total -= f.size;
    }
    await write.done;
  }
}

/** What to say of a file turned down: what a proxy in front of the server says of a large one is
 * not the server's words, and a server that has nowhere to upload to is from before attachments. */
const refusal = (e: ApiError) =>
  e.status === 413 ? "文件超过服务器允许的大小" : e.status === 404 ? "服务器的版本太旧，还不支持附件，请先升级服务器" : e.message;
