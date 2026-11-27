// Adding files as attachments (docs/design/attachments.md, docs/web.md#附件): what they are called, photos
// made smaller as the server has it, and their bytes kept on the device for the outbox to upload.

import { v7 as uuidv7 } from "uuid";
import { ApiError, api } from "./api/http";
import type { AiJobId } from "./api/types/AiJobId";
import type { Op } from "./api/types/Op";
import type { ServerSettings } from "./api/types/ServerSettings";
import type { TaskId } from "./api/types/TaskId";
import type { Staged } from "./sync/drafts";

/** What is shown as a picture, as a thumbnail and in the viewer; anything else, SVG too, is a file. */
export const PICTURES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp", "image/avif"]);
export const isPicture = (type: string) => PICTURES.has(type);

/** The largest file an attachment can have: `MAX_FILE_SIZE` in crates/core/src/model/attachment.rs. */
export const MAX_FILE_SIZE = 20 * 1024 * 1024;

const SETTINGS = "nexttodo.serverSettings";

/** The server's settings as last fetched; photos are made smaller until they have been. */
export const serverSettings = (): ServerSettings => JSON.parse(localStorage.getItem(SETTINGS) ?? '{"compress_images":true}');

export const keepServerSettings = (s: ServerSettings) => localStorage.setItem(SETTINGS, JSON.stringify(s));

/** Fetches them as a session opens; offline, the last ones stay. */
export async function fetchServerSettings() {
  try {
    keepServerSettings(await api<ServerSettings>("GET", "/settings"));
  } catch (e) {
    if (!(e instanceof ApiError)) throw e;
  }
}

/** Names systems give pictures, which say nothing of what is in them: a fixed prefix, then only
 * numbers, dates and times, on Android the app's package, and a copy's number. */
const PREFIX = /^(?:image|img|pxl|dsc|dscn|photo|screenshot|screen shot|截屏|屏幕截图|微信图片|mmexport|wx_camera)/i;
const REST = /^(?:[\d\s_\-.:]|at\b|[ap]m\b|上午|下午)*(?:_com(?:\.\w+)+)?(?:\s?\(\d+\))?$/i;

/** Whether a picture's name, without its extension, is one a system made up: nothing but the prefix
 * and what comes after it, or without the prefix only numbers. */
export const madeUp = (base: string) => REST.test(base.replace(PREFIX, "")) && (PREFIX.test(base) || !/[^\d\s_\-.:]/.test(base));

/** `name` split before its extension; an extension longer than 16 is more likely part of the name. */
export function split(name: string): [string, string] {
  const i = name.lastIndexOf(".");
  return i > 0 && name.length - i <= 16 ? [name.slice(0, i), name.slice(i)] : [name, ""];
}

const EXTENSIONS: Record<string, string> = { "image/jpeg": ".jpg", "image/svg+xml": ".svg" };

/** What a file added is called: its own name, unless that is one a system made up for a picture, or
 * there is none; then `图片 3.png`, the number after the highest such among `taken`, the names already
 * there. Cleaned to be one (docs/design/attachments.md). */
export function nameFor(name: string, type: string, taken: string[]): string {
  const cleaned = name.replace(/[\p{Cc}]/gu, "").replace(/[/\\]/g, "_").trim();
  const [base, ext] = split(cleaned === "." || cleaned === ".." ? "" : cleaned);
  const image = type.startsWith("image/");
  if (base && !(image && madeUp(base))) {
    // At most 200 characters, the extension kept.
    const chars = [...base];
    return chars.length + [...ext].length > 200 ? chars.slice(0, 200 - [...ext].length).join("") + ext : base + ext;
  }
  const sub = type.split("/")[1];
  const kind = image ? "图片" : "文件";
  const numbered = new RegExp(`^${kind} (\\d+)(\\.|$)`);
  const last = Math.max(0, ...taken.map((n) => Number(numbered.exec(n)?.[1] ?? 0)));
  return `${kind} ${last + 1}${ext || (EXTENSIONS[type] ?? (/^[a-z0-9]+$/.test(sub) ? `.${sub}` : ""))}`;
}

/** A file's type as the server takes it, `类型/子类型`; what the system does not know is just bytes. */
function mediaType(type: string): string {
  const t = type.split(";")[0].trim().toLowerCase();
  return /^[a-z0-9!#$&\-^_.+]+\/[a-z0-9!#$&\-^_.+]+$/.test(t) && t.length <= 100 ? t : "application/octet-stream";
}

const LONG_EDGE = 2560;
const QUALITY = 0.85;

/** A JPEG photo encoded again, its long edge at most `LONG_EDGE`: smaller, and without the place it was
 * taken. `<img>` turns it the way its EXIF says, as drawing it does. One the browser cannot read goes
 * as it is. */
async function shrink(file: Blob): Promise<Blob> {
  const url = URL.createObjectURL(file);
  try {
    const img = new Image();
    img.src = url;
    try {
      await img.decode();
    } catch {
      return file;
    }
    const scale = Math.min(1, LONG_EDGE / Math.max(img.naturalWidth, img.naturalHeight));
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(img.naturalWidth * scale);
    canvas.height = Math.round(img.naturalHeight * scale);
    canvas.getContext("2d")!.drawImage(img, 0, 0, canvas.width, canvas.height);
    return await new Promise<Blob>((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("toBlob gave nothing"))), "image/jpeg", QUALITY));
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** A file to be added, named and given its id before anything is done with it: what refers to it can
 * at once. */
export interface Planned {
  id: string;
  name: string;
  type: string;
  file: File;
  /** A photo made smaller first, as the server has it. */
  shrink: boolean;
}

export const tooLarge = (name: string) => `“${name}”超过 20 MB，不能添加`;

/** How large a file may be added: `max` bytes, but for a photo of type `shrunk`, which is made smaller
 * first and may come under it then. */
export const sizeLimit = (): { max: number; shrunk?: string } => ({ max: MAX_FILE_SIZE, ...(serverSettings().compress_images && { shrunk: "image/jpeg" }) });

/** Names `files` among `taken` and gives them ids, in the order given, which is the order they are
 * added in. Tells `refuse` of those that cannot be added, which are left out. */
export function plan(files: File[], taken: string[], refuse: (text: string) => void): Planned[] {
  const planned: Planned[] = [];
  const { max, shrunk } = sizeLimit();
  for (const file of files) {
    const type = mediaType(file.type);
    const name = nameFor(file.name, type, [...taken, ...planned.map((p) => p.name)]);
    const shrink = type === shrunk;
    if (!file.size) refuse(`“${name}”是空文件，不能添加`);
    else if (file.size > max && !shrink) refuse(tooLarge(name));
    else planned.push({ id: uuidv7(), name, type, file, shrink });
  }
  return planned;
}

/** What is kept of a planned file and uploaded: a photo made smaller first. Throws, saying so, when it
 * is still too large. */
export async function bytesOf(p: Planned): Promise<Blob> {
  const blob = p.shrink ? await shrink(p.file) : p.file;
  if (blob.size > MAX_FILE_SIZE) throw new Error(tooLarge(p.name));
  return blob;
}

/** The pictures an AI job goes with when the model sees them, in the order added: of the kinds models
 * take, up to a size and a number. As `seen` in crates/server/src/ai/organize.rs; both are checked
 * against fixtures/ai-pictures.json. */
const SENT_PICTURES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
const MAX_SENT_SIZE = 5 * 1024 * 1024;
const MAX_SENT = 10;

/** Why the model does not see each of the pictures among `files` an AI job goes with, by id, when
 * `vision` says whether it sees pictures at all (docs/ai.md#附件). */
export function unseen(files: Staged[], vision: boolean): Map<string, string> {
  const why = new Map<string, string>();
  let sent = 0;
  // In the order the server takes them: by id, which is when each was added.
  for (const f of [...files].sort((a, b) => (a.id < b.id ? -1 : 1))) {
    if (!isPicture(f.type)) continue;
    if (!vision) why.set(f.id, "未打开“识别图片”");
    else if (!SENT_PICTURES.has(f.type)) why.set(f.id, "仅支持 PNG、JPEG、WebP 和 GIF 图片");
    else if (f.size > MAX_SENT_SIZE) why.set(f.id, "图片超过 5 MB");
    else if (sent === MAX_SENT) why.set(f.id, "一次最多发 10 张图片");
    else sent++;
  }
  return why;
}

/** Whether the model sees any of the pictures among `files`: then an AI job needs no text. */
export function seesAny(files: Staged[], vision: boolean): boolean {
  const hidden = unseen(files, vision);
  return files.some((f) => isPicture(f.type) && !hidden.has(f.id));
}

/** The ops adding `files` to a task or an AI job. */
export const addOps = (files: Staged[], to: { task_id: TaskId } | { job_id: AiJobId }): Op[] =>
  files.map((f) => ({ op: "attachment.add", id: f.id, name: f.name, type: f.type, size: f.size, ...to }));

/** A reference to an attachment in a description: a Markdown link. */
export const reference = (a: { id: string; name: string }) => `[${a.name.replace(/[[\]\\]/g, "\\$&")}](attachment:${a.id})`;

/** The files in what was pasted or dropped. */
export const filesIn = (data: DataTransfer | null): File[] => [...(data?.files ?? [])];

/** Whether what is dragged has files. */
export const draggingFiles = (e: DragEvent | React.DragEvent): boolean => !!e.dataTransfer?.types.includes("Files");

/** Files dropped where nothing takes them aren't taken: the browser, and the app's web view, would
 * open the file in place of the app. */
export function refuseStrayDrops() {
  const refuse = (e: DragEvent) => {
    if (!draggingFiles(e) || e.defaultPrevented) return;
    e.preventDefault();
    e.dataTransfer!.dropEffect = "none";
  };
  addEventListener("dragover", refuse);
  addEventListener("drop", refuse);
}
