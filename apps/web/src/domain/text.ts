// The same checks the server makes on text (`crates/core/src/model.rs`), so the app can say what is
// wrong before anything is sent. Each returns the cleaned value or an error message.

export type Checked = { ok: true; value: string } | { ok: false; error: string };

const length = (s: string) => [...s].length;

/** Control characters but line breaks and tabs, which the server refuses: dropped, as they can't be
 * seen to be taken out. Some chat apps, QQ on Android among them, copy them in place of emoji. */
const control = /[^\P{Cc}\n\r\t]/gu;

function line(s: string, what: string, max: number): Checked {
  // A tab on one line is a space: between cells pasted from a spreadsheet, say.
  const value = s.replace(control, "").replace(/\t/g, " ").trim();
  if (!value) return { ok: false, error: `${what}不能为空` };
  if (/[\n\r]/.test(value)) return { ok: false, error: `${what}不能换行` };
  if (length(value) > max) return { ok: false, error: `${what}不能超过 ${max} 个字` };
  return { ok: true, value };
}

export const checkTitle = (s: string) => line(s, "标题", 200);

export function checkFolderName(s: string): Checked {
  const c = line(s, "名称", 100);
  return c.ok && c.value.includes("/") ? { ok: false, error: "文件夹名称不能包含 /" } : c;
}

export function checkDescription(s: string): Checked {
  const value = s.replace(control, "");
  if (length(value) > 20_000) return { ok: false, error: "描述不能超过 20000 个字" };
  return { ok: true, value };
}

/** An attachment's name, which is the file's name when it is saved: not a path. */
export function checkFileName(s: string): Checked {
  const c = line(s, "名称", 200);
  if (!c.ok) return c;
  if (/[/\\]/.test(c.value)) return { ok: false, error: "名称不能包含 / 或 \\" };
  if (c.value === "." || c.value === "..") return { ok: false, error: "名称不能是 . 或 .." };
  return c;
}

/** Text for AI to organize: long and on many lines, as written. */
export function checkJobText(s: string): Checked {
  const value = s.replace(control, "");
  if (!value.trim()) return { ok: false, error: "内容不能为空" };
  if (length(value) > 10_000) return { ok: false, error: "内容不能超过 10000 个字" };
  return { ok: true, value };
}

/** A note for AI organizing in a folder: empty clears it. */
export function checkNoteText(s: string): Checked {
  const value = s.replace(control, "").trim();
  if (length(value) > 2000) return { ok: false, error: "说明不能超过 2000 个字" };
  return { ok: true, value };
}

export const checkDisplayName = (s: string) => line(s, "显示名", 50);

export const checkTokenName = (s: string) => line(s, "令牌名", 50);
