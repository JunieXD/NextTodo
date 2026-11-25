import { expect, test } from "vitest";
import { readBackup } from "./transfer";

const NOT_OURS = "这不是 NextTodo 的备份，或者文件已损坏";
const ID = "019a4b2c-0000-7000-8000-000000000001";
type Bytes = Uint8Array<ArrayBuffer>;
const bytes = (s: string): Bytes => new TextEncoder().encode(s);
const DB = new Uint8Array([...bytes("SQLite format 3\0"), 1, 2, 3]);

/** A tar entry's header, its fields as given; the checksum made last unless given. */
function header(name: string, size: string, o: { prefix?: string; type?: string; checksum?: string } = {}) {
  const h = new Uint8Array(512);
  const put = (at: number, s: string) => h.set(bytes(s), at);
  put(0, name);
  put(100, "0000644\0");
  put(124, `${size}\0`);
  put(156, o.type ?? "0");
  put(257, "ustar\0");
  put(263, "00");
  if (o.prefix) put(345, o.prefix);
  put(148, "        ");
  put(148, o.checksum ?? `${h.reduce((n, b) => n + b, 0).toString(8).padStart(6, "0")}\0 `);
  return h;
}

/** A tar of `entries`, each padded to whole blocks, ending in two blocks of zeros. */
function tar(...entries: [Bytes, Bytes][]) {
  return new Blob(entries.flatMap(([h, data]) => [h, data, new Uint8Array((512 - (data.length % 512)) % 512)]).concat(new Uint8Array(1024)));
}

const entry = (name: string, data: Bytes): [Bytes, Bytes] => [header(name, data.length.toString(8).padStart(11, "0")), data];
const manifest = (format = 1) => entry("backup.json", bytes(JSON.stringify({ format })));
const text = async (b: Blob) => new TextDecoder().decode(await b.arrayBuffer());

test("a backup's files come in order, its database apart", async () => {
  const b = await readBackup(tar(manifest(), entry(`files/${ID}`, bytes("milk")), entry("nexttodo.db", DB)));
  expect(await Promise.all(b.files.map(async ([id, f]) => [id, await text(f)]))).toEqual([[ID, "milk"]]);
  expect(new Uint8Array(await b.database.arrayBuffer())).toEqual(DB);
});

test("a database alone is a backup from before there were attachments", async () => {
  const b = await readBackup(new Blob([DB]));
  expect(b.files).toEqual([]);
  expect(b.database.size).toBe(DB.length);
});

test("a backup a newer version wrote asks for an update", async () => {
  await expect(readBackup(tar(manifest(2), entry("nexttodo.db", DB)))).rejects.toThrow("备份来自更新版本的 NextTodo，请先更新服务端");
});

test("anything else is not a backup, and is read no further", async () => {
  const db = entry("nexttodo.db", DB);
  const whole = tar(manifest(), db);
  const odd = [
    // Damaged headers: a size not in octal, going back, or a checksum that does not add up.
    tar(manifest(), [header("nexttodo.db", "-0000001750"), DB]),
    tar(manifest(), [header("nexttodo.db", "0000000002x"), DB]),
    tar(manifest(), [header("nexttodo.db", DB.length.toString(8).padStart(11, "0"), { checksum: "0000000\0 " }), DB]),
    // Cut short within the database or its padding, or after a part of a header.
    whole.slice(0, 1024 + 10),
    whole.slice(0, 1024 + 512),
    new Blob([whole, new Uint8Array(100)]).slice(0, 2048 + 100),
    // Named otherwise than it reads, or not a file.
    tar(manifest(), [header("nexttodo.db", DB.length.toString(8).padStart(11, "0"), { prefix: "x" }), DB]),
    tar(manifest(), [header("nexttodo.db", DB.length.toString(8).padStart(11, "0"), { type: "2" }), DB]),
    // Without the manifest first, with a name it does not have, twice the same, or no database.
    tar(db, manifest()),
    tar(manifest(), entry("notes.txt", bytes("x")), db),
    tar(manifest(), entry("files/not-an-id", bytes("x")), db),
    tar(manifest(), db, db),
    tar(manifest(), entry(`files/${ID}`, bytes("milk")), entry(`files/${ID}`, bytes("eggs")), db),
    tar(manifest(), entry(`files/${ID}`, bytes("milk")), entry(`files/${ID.toUpperCase()}`, bytes("eggs")), db),
    tar(manifest(), entry(`files/${ID}`, bytes("milk"))),
  ];
  for (const [i, b] of odd.entries()) await expect(readBackup(b), `#${i}`).rejects.toThrow(NOT_OURS);
});
