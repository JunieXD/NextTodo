// Moving data between local mode and a server account (docs/local.md#搬到服务器和搬回本机): reading a
// backup to import, and importing it. The export is `fetchExport`.

import { v7 as uuidv7, validate, version } from "uuid";
import { api, fileRequest } from "./api/http";
import type { ImportBackup } from "./api/types/ImportBackup";
import type { Imported } from "./api/types/Imported";
import { MAX_FILE_SIZE } from "./attach";

const NOT_OURS = "这不是 NextTodo 的备份，或者文件已损坏";

/** How backups are written that this version reads: `BACKUP_FORMAT` in crates/core/src/model/mod.rs. */
const BACKUP_FORMAT = 1;

/** Why an import stops when the device is no longer signed in as it was: its files would go to
 * another account. */
const SIGNED_OUT = "已退出登录，导入已停止";

/** How long the server may take to add a backup once its files are up. */
const IMPORT_TIMEOUT_MS = 5 * 60_000;

/** What a backup has to upload: its database, and its attachments' files by their ids there. */
export type Backup = { database: Blob; files: [string, Blob][] };

/** The backup in `file`, checked as a restore checks it before reading the database: a tar of its
 * manifest first, attachments' files and the database; or a database alone, as backups were before
 * there were attachments. Throws what to tell the user. */
export async function readBackup(file: Blob): Promise<Backup> {
  const text = async (b: Blob) => new TextDecoder().decode(await b.arrayBuffer());
  if ((await text(file.slice(0, 16))) === "SQLite format 3\0") return sized({ database: file, files: [] });
  const entries: [string, Blob][] = [];
  let at = 0;
  while (at < file.size) {
    if (at + 512 > file.size) throw new Error(NOT_OURS);
    const header = new Uint8Array(await file.slice(at, at + 512).arrayBuffer());
    // The end: blocks of zeros.
    if (header.every((b) => b === 0)) break;
    const field = (from: number, to: number) => new TextDecoder().decode(header.subarray(from, to)).split("\0")[0];
    const octal = (from: number, to: number) => (/^[0-7]+$/.test(field(from, to).trim()) ? parseInt(field(from, to).trim(), 8) : NaN);
    // Of all its bytes, its own counted as spaces.
    const sum = header.reduce((n, b, i) => n + (i >= 148 && i < 156 ? 32 : b), 0);
    const size = octal(124, 136);
    const end = at + 512 + Math.ceil(size / 512) * 512;
    // A file's, whole, named in full: no links, directories, long names or name prefixes.
    const plain = (header[156] === 0 || header[156] === 0x30) && !(field(257, 263) === "ustar" && field(345, 500));
    if (octal(148, 156) !== sum || !plain || !(end <= file.size)) throw new Error(NOT_OURS);
    entries.push([field(0, 100), file.slice(at + 512, at + 512 + size)]);
    at = end;
  }
  const [manifest, ...rest] = entries;
  if (manifest?.[0] !== "backup.json" || manifest[1].size > 1024) throw new Error(NOT_OURS);
  let format: unknown;
  try {
    format = (JSON.parse(await text(manifest[1])) as { format?: unknown }).format;
  } catch {
    throw new Error(NOT_OURS);
  }
  if (typeof format !== "number") throw new Error(NOT_OURS);
  if (format > BACKUP_FORMAT) throw new Error("备份来自更新版本的 NextTodo，请先更新服务端");
  const files = new Map<string, Blob>();
  let database: Blob | undefined;
  for (const [name, blob] of rest) {
    const id = name.startsWith("files/") ? name.slice("files/".length) : null;
    // One id however it is written, as the server reads it.
    if (id !== null && validate(id) && version(id) === 7 && !files.has(id.toLowerCase()) && blob.size <= MAX_FILE_SIZE) files.set(id.toLowerCase(), blob);
    else if (name === "nexttodo.db" && !database) database = blob;
    else throw new Error(NOT_OURS);
  }
  if (!database) throw new Error(NOT_OURS);
  return sized({ database, files: [...files] });
}

function sized(b: Backup): Backup {
  if (b.database.size > MAX_FILE_SIZE) throw new Error("备份里的数据库超过 20 MB，暂时导入不了");
  return b;
}

/** Uploads the backup's files, each under a new id made in their order, telling `progress` how many
 * of how many are up: what `POST /import` is to be given, with the id of the change to make. Stops,
 * failing, once it no longer goes as it did when it began, as `current` says. */
export async function uploadBackup(b: Backup, progress: (done: number, all: number) => void, current: () => boolean): Promise<ImportBackup> {
  const stop = new AbortController().signal;
  const all = b.files.length + 1;
  let done = 0;
  const upload = async (blob: Blob, type: string) => {
    if (!current()) throw new Error(SIGNED_OUT);
    const id = uuidv7();
    await fileRequest("PUT", id, stop, { blob, type });
    progress(++done, all);
    return id;
  };
  const files: Record<string, string> = {};
  // Their type is the backup database's to say.
  for (const [old, blob] of b.files) files[old] = await upload(blob, "application/octet-stream");
  const database = await upload(b.database, "application/vnd.sqlite3");
  return { id: uuidv7(), database, files };
}

/** Adds the backup uploaded to the account as change `body.id`, which undoing takes back: what it
 * added. Sent again, it is added once; not once it no longer goes as the upload did, as `current`
 * says. */
export async function sendImport(body: ImportBackup, current: () => boolean): Promise<Imported> {
  if (!current()) throw new Error(SIGNED_OUT);
  return api<Imported>("POST", "/import", body, IMPORT_TIMEOUT_MS);
}
