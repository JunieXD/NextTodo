// What a tidying's suggestions would still do, part by part, and the change taking some of them makes.
// See docs/ai.md#整理文件夹.

import type { AiJob } from "../api/types/AiJob";
import type { Due } from "../api/types/Due";
import type { FolderId } from "../api/types/FolderId";
import type { NewTask } from "../api/types/NewTask";
import type { Op } from "../api/types/Op";
import type { Suggestion } from "../api/types/Suggestion";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";
import { formatDue, isoDate, remindLabel } from "./dates";
import { rekey } from "./reorder";
import { repeatLabel } from "./repeat";

type Fields = { title?: string; description?: string; due?: Due | null; done?: boolean; folder_id?: FolderId };

/** What taking a part does. */
type Act =
  | { update: TaskId; fields: Fields }
  | { link: [before: TaskId, after: TaskId] }
  | { unlink: [before: TaskId, after: TaskId] }
  | { remove: TaskId }
  | { create: NewTask }
  | { order: TaskId[] };

/** One change a suggestion makes. When what it changes has changed since the model saw it, `now`
 * says so, and `current` holds what is there now. */
export type Part = { key: string; label: string; text: string; now?: string; current?: string; act: Act };

/** What is suggested for a task, a new task, or the order. */
export type Group = { key: string; heading: string; reason: string; parts: Part[] };

const sameDue = (a: Due | null, b: Due | null) =>
  a?.date === b?.date &&
  a?.time === b?.time &&
  a?.remind_before === b?.remind_before &&
  a?.repeat?.every === b?.repeat?.every &&
  a?.repeat?.unit === b?.repeat?.unit &&
  a?.repeat?.from === b?.repeat?.from;

/** `11月20日 18:00 · 提前 30 分钟 · 每周`, or `无`. */
const dueText = (d: Due | null, now: Date) =>
  d ? [formatDue(d, now), d.remind_before !== undefined && remindLabel(d.remind_before), d.repeat && repeatLabel(d.repeat)].filter(Boolean).join(" · ") : "无";

/** What `s` would still do: to tasks there and in the state it found them, and links not made yet. */
function group(s: Suggestion, j: AiJob, board: Board, created: Map<TaskId, string>): Group | null {
  const now = new Date();
  const name = (id: TaskId) => {
    const t = board.task(id);
    return t ? `#${t.number} ${t.title}` : `“${created.get(id)}”`;
  };
  if ("order" in s) {
    const open = board.tasksIn(j.folder_id).filter((t) => t.done_at === null);
    const tasks = s.order.flatMap((id) => open.filter((t) => t.id === id));
    const placed = open.filter((t) => tasks.includes(t));
    if (tasks.length < 2 || tasks.every((t, i) => t === placed[i])) return null;
    const text = tasks.map((t, i) => `${i + 1}. #${t.number} ${t.title}`).join("\n");
    return { key: "order", heading: "调整顺序", reason: s.reason, parts: [{ key: "order", label: "新的顺序", text, act: { order: tasks.map((t) => t.id) } }] };
  }
  if ("create" in s) {
    const text = [
      s.title,
      s.description,
      s.due && `截止：${dueText(s.due, now)}`,
      s.after.length && `前置：${s.after.map(name).join("、")}`,
      s.before.length && `后续：${s.before.map(name).join("、")}`,
    ]
      .filter(Boolean)
      .join("\n");
    return { key: s.create, heading: "新建事项", reason: s.reason, parts: [{ key: "create", label: "新建", text, act: { create: s } }] };
  }
  const id = "task" in s ? s.task : s.delete;
  const t = board.task(id);
  const heading = t && `#${t.number} ${t.title}`;
  if ("delete" in s) {
    if (!t) return null;
    const changed = t.title !== s.was.title || t.description !== s.was.description || !sameDue(t.due, s.was.due);
    const current = JSON.stringify([t.title, t.description, t.due]);
    const part: Part = { key: "delete", label: "删除", text: "删除这个事项", act: { remove: id }, ...(changed && { now: "整理后改过这个事项", current }) };
    return { key: id, heading: heading!, reason: s.reason, parts: [part] };
  }
  // Done since, it needs none of it, unless it was to be opened again.
  if (!t || (t.done_at !== null && s.done !== false)) return null;
  const parts: Part[] = [];
  // A new title and description go together: the title may have left details to the description.
  const text = { ...(s.title !== undefined && { title: s.title }), ...(s.description !== undefined && { description: s.description }) };
  if (text.title !== undefined || text.description !== undefined) {
    const label = text.description === undefined ? "标题" : text.title === undefined ? "描述" : "标题和描述";
    const show = (title: string, description: string) =>
      [text.title !== undefined && title, text.description !== undefined && (description || "（空）")].filter((x) => x !== false).join("\n\n");
    const changed = (text.title !== undefined && t.title !== s.was.title) || (text.description !== undefined && t.description !== s.was.description);
    const current = JSON.stringify([text.title !== undefined && t.title, text.description !== undefined && t.description]);
    const was = changed && { now: `整理后已改为：${show(t.title, t.description)}`, current };
    parts.push({ key: "text", label, text: show(text.title ?? "", text.description ?? ""), act: { update: id, fields: text }, ...was });
  }
  if (s.due !== undefined && !sameDue(t.due, s.due)) {
    const changed = !sameDue(t.due, s.was.due);
    const was = changed && { now: `整理后已改为：${dueText(t.due, now)}`, current: JSON.stringify(t.due) };
    parts.push({ key: "due", label: "截止", text: `${dueText(s.was.due, now)} → ${dueText(s.due, now)}`, act: { update: id, fields: { due: s.due } }, ...was });
  }
  if (s.done !== undefined && s.done !== (t.done_at !== null)) {
    const text = !s.done ? "重新打开" : t.due?.repeat ? "标为完成，截止时间顺延到下一次" : "标为完成";
    parts.push({ key: "done", label: "状态", text, act: { update: id, fields: { done: s.done } } });
  }
  if (s.folder !== undefined && board.folder(s.folder) && t.folder_id !== s.folder) {
    const moved = t.folder_id !== j.folder_id && { now: `整理后已移到“${board.place(t.folder_id)}”`, current: t.folder_id ?? "" };
    parts.push({ key: "folder", label: "移动", text: `移到“${board.place(s.folder)}”`, act: { update: id, fields: { folder_id: s.folder } }, ...moved });
  }
  const before = board.before(t);
  for (const b of s.after) {
    if (before.some((x) => x.id === b) || !(board.task(b) || created.has(b))) continue;
    parts.push({ key: `after ${b}`, label: "前置", text: name(b), act: { link: [b, id] } });
  }
  for (const b of s.unlink ?? []) {
    if (!before.some((x) => x.id === b)) continue;
    parts.push({ key: `unlink ${b}`, label: "去掉前置", text: name(b), act: { unlink: [b, id] } });
  }
  return parts.length ? { key: id, heading: heading!, reason: s.reason, parts } : null;
}

/** The operations taking `parts` makes, the job's deletion last. */
export function ops(parts: Part[], j: AiJob, board: Board): Op[] {
  const creates: Op[] = [];
  const made = new Set<TaskId>();
  const removed = new Set<TaskId>();
  const updates = new Map<TaskId, Fields>();
  const links: [TaskId, TaskId][] = [];
  const unlinks: [TaskId, TaskId][] = [];
  let order: TaskId[] = [];
  for (const { act: a } of parts) {
    if ("create" in a) {
      const c = a.create;
      made.add(c.create);
      creates.push({ op: "task.create", id: c.create, title: c.title, ...(c.description && { description: c.description }), ...(c.due && { due: c.due }), ...(j.folder_id && { folder_id: j.folder_id }) });
      links.push(...c.after.map((b): [TaskId, TaskId] => [b, c.create]), ...c.before.map((x): [TaskId, TaskId] => [c.create, x]));
    } else if ("update" in a) updates.set(a.update, { ...updates.get(a.update), ...a.fields });
    else if ("remove" in a) removed.add(a.remove);
    else if ("link" in a) links.push(a.link);
    else if ("unlink" in a) unlinks.push(a.unlink);
    else order = a.order;
  }
  // A link to a new task left out, or to a task deleted, goes with it; its deletion takes its links.
  const there = (id: TaskId) => made.has(id) || (board.task(id) !== undefined && !removed.has(id));
  const today = isoDate(new Date());
  const out: Op[] = [
    ...creates,
    ...[...updates].map(([id, f]): Op => ({ op: "task.update", id, ...f, ...(f.done && { today }) })),
    ...[...removed].map((id): Op => ({ op: "task.delete", id })),
    // Taken away before others are added: turned round, a link would otherwise close a cycle.
    ...unlinks.filter(([b, a]) => there(b) && there(a)).map(([before, after]): Op => ({ op: "dep.remove", before, after })),
    ...[...new Map(links.filter(([b, a]) => there(b) && there(a)).map((l) => [`${l[0]}>${l[1]}`, l])).values()].map(
      ([before, after]): Op => ({ op: "dep.add", before, after }),
    ),
  ];
  if (order.length) {
    // The tasks still in the folder, those in the order taking the places they hold in it, in its sequence.
    const list = board.tasksIn(j.folder_id).filter((t) => t.done_at === null && !removed.has(t.id) && updates.get(t.id)?.folder_id === undefined);
    const queue = order.flatMap((id) => list.filter((t) => t.id === id));
    const wanted = list.map((t) => (order.includes(t.id) ? queue.shift()! : t));
    out.push(...rekey(wanted, "").map((k): Op => ({ op: "task.update", ...k })));
  }
  out.push({ op: "ai_job.delete", id: j.id });
  return out;
}

/** What `j`'s suggestions would still do, those with nothing left to do left out. */
export function groups(j: AiJob, board: Board): Group[] {
  const suggestions = j.suggestions ?? [];
  const created = new Map(suggestions.flatMap((s) => ("create" in s ? [[s.create, s.title] as const] : [])));
  return suggestions.map((s) => group(s, j, board, created)).filter((g) => g !== null);
}
