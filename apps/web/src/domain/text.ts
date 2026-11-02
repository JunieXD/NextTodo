// The same checks the server makes on text (`crates/core/src/model.rs`), so the app can say what is
// wrong before anything is sent. Each returns the cleaned value or an error message.

export type Checked = { ok: true; value: string } | { ok: false; error: string };

const length = (s: string) => [...s].length;

function line(s: string, what: string, max: number): Checked {
  const value = s.trim();
  if (!value) return { ok: false, error: `${what}不能为空` };
  if (/[\n\r]/.test(value)) return { ok: false, error: `${what}只能有一行` };
  if (/\p{Cc}/u.test(value)) return { ok: false, error: `${what}不能包含控制字符` };
  if (length(value) > max) return { ok: false, error: `${what}不能超过 ${max} 个字` };
  return { ok: true, value };
}

export const checkTitle = (s: string) => line(s, "标题", 200);

export function checkFolderName(s: string): Checked {
  const c = line(s, "名称", 100);
  return c.ok && c.value.includes("/") ? { ok: false, error: "文件夹名称里不能有 /" } : c;
}

export function checkDescription(s: string): Checked {
  if (length(s) > 20_000) return { ok: false, error: "描述不能超过 20000 个字" };
  if (/[^\P{Cc}\n\r\t]/u.test(s)) return { ok: false, error: "描述不能包含控制字符" };
  return { ok: true, value: s };
}

export const checkDisplayName = (s: string) => line(s, "显示名", 50);

export const checkTokenName = (s: string) => line(s, "令牌名", 50);
