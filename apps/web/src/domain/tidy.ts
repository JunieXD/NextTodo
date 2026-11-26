// What a tidying's suggestions would still do, part by part, and the change taking some of them makes.
// See docs/ai.md#整理文件夹.

import type { AiJob } from "../api/types/AiJob";
import type { Due } from "../api/types/Due";
import type { DueAfter } from "../api/types/DueAfter";
import type { FolderColor } from "../api/types/FolderColor";
import type { FolderIcon } from "../api/types/FolderIcon";
import type { FolderId } from "../api/types/FolderId";
import type { NewTask } from "../api/types/NewTask";
import type { Op } from "../api/types/Op";
import type { Remind } from "../api/types/Remind";
import type { RemindAt } from "../api/types/RemindAt";
import type { Suggestion } from "../api/types/Suggestion";
import type { TaskId } from "../api/types/TaskId";
import type { TaskFields } from "./apply";
import { Board } from "./board";
import { dueAfterLabel, formatDue, isoDate, remindAtLabel, remindLabel, startRemindLabel } from "./dates";
import { arrange } from "./reorder";
import { lastTime, repeatText } from "./repeat";

/** What taking a part does. */
type Act =
  | { update: TaskId; fields: TaskFields }
  | { link: [before: TaskId, after: TaskId] }
  | { unlink: [before: TaskId, after: TaskId] }
  | { remove: TaskId }
  | { create: NewTask }
  | { order: TaskId[] }
  | { restyle: FolderId; color?: FolderColor | null; icon?: FolderIcon | null };

/** A folder's colour or icon as it was and is to be, by its code, `null` the default. */
export type LookChange = { field: "color" | "icon"; was: string | null; to: string | null };

/** One change a suggestion makes, `text` in words or a folder's look. When what it changes has changed
 * since the model saw it, `now` says so, and `current` holds what is there now. */
export type Part = { key: string; label: string; text: string | LookChange; now?: string; current?: string; act: Act };

/** What is suggested for a task, a new task, the order, or a folder's look. */
export type Group = { key: string; heading: string; reason: string; parts: Part[] };

const sameDue = (a: Due | null, b: Due | null) =>
  a?.date === b?.date &&
  a?.time === b?.time &&
  JSON.stringify(a?.reminders) === JSON.stringify(b?.reminders) &&
  a?.repeat?.every === b?.repeat?.every &&
  a?.repeat?.unit === b?.repeat?.unit &&
  a?.repeat?.from === b?.repeat?.from &&
  a?.repeat?.days?.join() === b?.repeat?.days?.join() &&
  !a?.repeat?.lunar === !b?.repeat?.lunar &&
  !a?.repeat?.legal === !b?.repeat?.legal &&
  a?.repeat?.until === b?.repeat?.until;

const sameAfter = (a: DueAfter | undefined, b: DueAfter | undefined) =>
  a?.days === b?.days && a?.time === b?.time && JSON.stringify(a?.reminders) === JSON.stringify(b?.reminders);

/** A task's due date, its reminders of all three kinds and its repeat, as a part shows them: `11月20日 18:00 ·
 * 前一天 20:00、开始前 30 分钟 · 每周`; a due date its prerequisites give with them, `前置完成后 3 天内`,
 * once they are done `11月20日 18:00（前置完成后 3 天内 18:00）`; `无` for none of it. */
type Timing = { due: Due | null; due_after?: DueAfter; start_reminders?: Remind[]; remind_at?: RemindAt[] };
function timingText({ due, due_after: after, start_reminders, remind_at }: Timing, now: Date): string {
  const when = !after ? due && formatDue(due, now) : due ? `${formatDue(due, now)}（${dueAfterLabel(after)}）` : dueAfterLabel(after);
  const reminders = [
    ...((after ?? due)?.reminders ?? []).map(remindLabel),
    ...(start_reminders ?? []).map(startRemindLabel),
    ...(remind_at ?? []).map((r) => remindAtLabel(r, now)),
  ];
  return [when, reminders.join("、"), due?.repeat && repeatText(due.repeat, due.date, now)].filter(Boolean).join(" · ") || "无";
}

/** Lists the same, none being as many as empty. */
const sameList = (a: unknown[] | undefined, b: unknown[] | undefined) => JSON.stringify(a ?? []) === JSON.stringify(b ?? []);

/** What `s` would still do: to tasks and folders there and in the state it found them, and links not
 * made yet. */
function group(s: Suggestion, j: AiJob, board: Board, created: Map<TaskId, string>): Group | null {
  const now = new Date();
  const name = (id: TaskId) => {
    const t = board.task(id);
    return t ? `#${t.number} ${t.title}` : `“${created.get(id)}”`;
  };
  if ("style" in s) {
    const f = board.folder(s.style);
    if (!f) return null;
    const part = (field: "color" | "icon", label: string, to: string | null | undefined, act: Act): Part[] => {
      const is = f[field] ?? null;
      if (to === undefined || to === is) return [];
      const changed = is !== s.was[field] && { now: `整理后改过${label}`, current: is ?? "" };
      return [{ key: field, label, text: { field, was: s.was[field], to }, act, ...changed }];
    };
    const parts = [...part("color", "颜色", s.color, { restyle: f.id, color: s.color }), ...part("icon", "图标", s.icon, { restyle: f.id, icon: s.icon })];
    return parts.length ? { key: s.style, heading: `文件夹“${board.place(f.id)}”`, reason: s.reason, parts } : null;
  }
  if ("order" in s) {
    const open = board.tasksIn(j.folder_id).filter((t) => t.done_at === null);
    const then = arrange(board, open, s.order).list;
    if (then.every((t, i) => t === open[i])) return null;
    const text = then
      .filter((t) => s.order.includes(t.id))
      .map((t, i) => `${i + 1}. #${t.number} ${t.title}`)
      .join("\n");
    return { key: "order", heading: "调整顺序", reason: s.reason, parts: [{ key: "order", label: "新的顺序", text, act: { order: s.order } }] };
  }
  if ("create" in s) {
    const text = [
      s.title,
      s.description,
      (s.due || s.remind_at) && `${s.due ? "截止" : "提醒"}：${timingText({ due: s.due ?? null, remind_at: s.remind_at }, now)}`,
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
    const changed =
      t.title !== s.was.title ||
      t.description !== s.was.description ||
      !sameDue(t.due, s.was.due) ||
      !sameAfter(t.due_after, s.was.due_after) ||
      !sameList(t.start_reminders, s.was.start_reminders) ||
      !sameList(t.remind_at, s.was.remind_at);
    const current = JSON.stringify([t.title, t.description, t.due, t.due_after, t.start_reminders, t.remind_at]);
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
  // The due date, the reminders and the repeat are one part, as the reminders given take the place of
  // all three kinds (docs/ai.md#整理文件夹); of those by the start, those its start still has what they go by for.
  // A date the prerequisites gave, set as a date of its own, is a change too.
  const given = s.start_reminders;
  const fits = given && t.start ? given.filter((r) => t.start_time || "days" in r) : undefined;
  // None left of some given is no change, not clearing those it has.
  const byStart = fits && (fits.length || !given?.length) ? fits : undefined;
  // As it was for what it sets, as it is for the rest.
  const from: Timing = {
    due: s.due === undefined ? t.due : s.was.due,
    due_after: s.due === undefined ? t.due_after : s.was.due_after,
    start_reminders: byStart ? s.was.start_reminders : t.start_reminders,
    remind_at: s.remind_at ? s.was.remind_at : t.remind_at,
  };
  const to: Timing = {
    due: s.due === undefined ? t.due : s.due,
    due_after: s.due === undefined ? t.due_after : undefined,
    start_reminders: byStart ?? t.start_reminders,
    remind_at: s.remind_at ?? t.remind_at,
  };
  if ((s.due !== undefined && (!sameDue(t.due, s.due) || t.due_after)) || !sameList(to.start_reminders, t.start_reminders) || !sameList(to.remind_at, t.remind_at)) {
    // Of what it sets, what has changed since.
    const changed =
      (s.due !== undefined && (!sameDue(t.due, s.was.due) || !sameAfter(t.due_after, s.was.due_after))) ||
      (byStart !== undefined && !sameList(t.start_reminders, s.was.start_reminders)) ||
      (s.remind_at !== undefined && !sameList(t.remind_at, s.was.remind_at));
    const current = JSON.stringify([s.due !== undefined && [t.due, t.due_after], byStart !== undefined && t.start_reminders, s.remind_at !== undefined && t.remind_at]);
    const was = changed && { now: `整理后已改为：${timingText(t, now)}`, current };
    const fields = { ...(s.due !== undefined && { due: s.due }), ...(byStart && { start_reminders: byStart }), ...(s.remind_at && { remind_at: s.remind_at }) };
    parts.push({ key: "due", label: s.due === undefined ? "提醒" : "截止", text: `${timingText(from, now)} → ${timingText(to, now)}`, act: { update: id, fields }, ...was });
  }
  if (s.done !== undefined && s.done !== (t.done_at !== null)) {
    // As the suggestion's own due date, taken with it by default, has it.
    const due = s.due === undefined ? t.due : s.due;
    const moves = due?.repeat && !lastTime(due, isoDate(now));
    const text = !s.done ? "重新打开" : moves ? "标为完成，截止时间顺延到下一次" : "标为完成";
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
  const updates = new Map<TaskId, TaskFields>();
  const links: [TaskId, TaskId][] = [];
  const unlinks: [TaskId, TaskId][] = [];
  const looks = new Map<FolderId, { color?: FolderColor | null; icon?: FolderIcon | null }>();
  let order: TaskId[] = [];
  for (const { act: a } of parts) {
    if ("create" in a) {
      const c = a.create;
      made.add(c.create);
      creates.push({ op: "task.create", id: c.create, title: c.title, ...(c.description && { description: c.description }), ...(c.due && { due: c.due }), ...(c.remind_at && { remind_at: c.remind_at }), ...(j.folder_id && { folder_id: j.folder_id }) });
      links.push(...c.after.map((b): [TaskId, TaskId] => [b, c.create]), ...c.before.map((x): [TaskId, TaskId] => [c.create, x]));
    } else if ("update" in a) updates.set(a.update, { ...updates.get(a.update), ...a.fields });
    else if ("remove" in a) removed.add(a.remove);
    else if ("link" in a) links.push(a.link);
    else if ("unlink" in a) unlinks.push(a.unlink);
    else if ("restyle" in a) {
      const { restyle, ...look } = a;
      looks.set(restyle, { ...looks.get(restyle), ...look });
    } else order = a.order;
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
    ...[...looks].map(([id, look]): Op => ({ op: "folder.update", id, ...look })),
  ];
  if (order.length) {
    const then = relinked(board, unlinks, links);
    const list = then.tasksIn(j.folder_id).filter((t) => t.done_at === null && !removed.has(t.id) && updates.get(t.id)?.folder_id === undefined);
    out.push(...arrange(then, list, order).keys.map((k): Op => ({ op: "task.update", ...k })));
  }
  out.push({ op: "ai_job.delete", id: j.id });
  return out;
}

/** What `j`'s suggestions would still do, those with nothing left to do left out; a new order as the
 * parts `taken` of the others leave the dependencies. */
export function groups(j: AiJob, board: Board, taken: (g: Group, p: Part) => boolean): Group[] {
  const suggestions = j.suggestions ?? [];
  const created = new Map(suggestions.flatMap((s) => ("create" in s ? [[s.create, s.title] as const] : [])));
  const rest = suggestions.map((s) => ("order" in s ? null : group(s, j, board, created)));
  const acts = rest.flatMap((g) => (g ? g.parts.filter((p) => taken(g, p)).map((p) => p.act) : []));
  const then = relinked(
    board,
    acts.flatMap((a) => ("unlink" in a ? [a.unlink] : [])),
    acts.flatMap((a) => ("link" in a ? [a.link] : [])),
  );
  return suggestions.map((s, i) => ("order" in s ? group(s, j, then, created) : rest[i])).filter((g) => g !== null);
}

/** `board` with the dependencies `unlinks` taken away and `links` added, those between tasks on it. */
function relinked(board: Board, unlinks: [TaskId, TaskId][], links: [TaskId, TaskId][]): Board {
  const deps = board.snapshot.dependencies.filter((d) => !unlinks.some(([b, a]) => d.before === b && d.after === a));
  for (const [before, after] of links) {
    if (board.task(before) && board.task(after) && !deps.some((d) => d.before === before && d.after === after)) deps.push({ before, after, created_at: 0 });
  }
  return new Board({ ...board.snapshot, dependencies: deps }, board.now, board.me);
}
