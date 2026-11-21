// Due dates as people read them. Dates and times are the user's local ones, as stored.

import type { Due } from "../api/types/Due";
import type { Remind } from "../api/types/Remind";
import type { RemindOn } from "../api/types/RemindOn";

const WEEK = "日一二三四五六";
const pad = (n: number) => String(n).padStart(2, "0");

/** `YYYY-MM-DD` of a moment, in local time. */
export const isoDate = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

/** `HH:MM` of a moment, in local time. */
export const isoTime = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** `HH:MM` of a time typed: `9:07`, `907`, or the hour alone, `9`; with a full-width colon or a dot as
 * well, as Chinese input methods give them. Null if it is none. */
export function parseTime(s: string): string | null {
  const m = /^(\d{1,2})(?:[:：.]?(\d{2}))?$/.exec(s.trim());
  if (!m) return null;
  const [h, min] = [Number(m[1]), Number(m[2] ?? 0)];
  return h < 24 && min < 60 ? `${pad(h)}:${pad(min)}` : null;
}

/** `2026-11-04 16:30 星期三`: the local time, as AI organizing reads "tomorrow" by. */
export const localTime = (d: Date) => `${isoDate(d)} ${isoTime(d)} 星期${WEEK[d.getDay()]}`;

/** The start of `date`, `YYYY-MM-DD`, in local time. */
export function day(date: string): Date {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** When it is due: the time given, or the end of the day for a date alone. */
export function deadline(due: Due): Date {
  const d = day(due.date);
  if (!due.time) return new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1);
  const [h, m] = due.time.split(":").map(Number);
  d.setHours(h, m);
  return d;
}

const daysFrom = (now: Date, date: string) =>
  Math.round((day(date).getTime() - new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) / 864e5);

export type DueKind = "overdue" | "today" | "week" | "later";

export function dueKind(due: Due, now: Date): DueKind {
  if (deadline(due) <= now) return "overdue";
  const days = daysFrom(now, due.date);
  return days === 0 ? "today" : days <= 7 ? "week" : "later";
}

/** `今天 18:00`, `明天`, `周五`, `11月20日 18:00`, `2027年3月15日`. */
export function formatDue(due: Due, now: Date): string {
  const time = due.time ? ` ${due.time}` : "";
  const days = daysFrom(now, due.date);
  if (days === 0) return `今天${time}`;
  if (days === -1) return `昨天${time}`;
  if (days === 1) return `明天${time}`;
  const d = day(due.date);
  const leftThisWeek = 7 - ((now.getDay() + 6) % 7);
  if (days > 1 && days < leftThisWeek) return `周${WEEK[d.getDay()]}${time}`;
  if (d.getFullYear() === now.getFullYear()) return `${d.getMonth() + 1}月${d.getDate()}日${time}`;
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日${time}`;
}

/** `20:00 开始`, `明天开始`, `周五 09:30 开始`, `11月14日开始`: when a task starts, written as due dates
 * are; today's has a time, or it would have started. */
export function startLabel(date: string, time: string | undefined, now: Date): string {
  if (time && date === isoDate(now)) return `${time} 开始`;
  return time ? `${formatDue({ date, time }, now)} 开始` : `${formatDue({ date }, now)}开始`;
}

/** The 42 days a month's calendar shows: six weeks from the Monday on or before its first day. */
export function monthGrid(year: number, month: number): Date[] {
  const start = 1 - ((new Date(year, month, 1).getDay() + 6) % 7);
  return Array.from({ length: 42 }, (_, i) => new Date(year, month, start + i));
}

/** `11月2日 星期一` */
export const formatToday = (now: Date) => `${now.getMonth() + 1}月${now.getDate()}日 星期${WEEK[now.getDay()]}`;

/** `今天`, `昨天`, `11月1日 周六`, with the year when it is not this year. */
export function formatDay(ms: number, now: Date): string {
  const d = new Date(ms);
  const days = daysFrom(now, isoDate(d));
  if (days === 0) return "今天";
  if (days === -1) return "昨天";
  const year = d.getFullYear() === now.getFullYear() ? "" : `${d.getFullYear()}年`;
  return `${year}${d.getMonth() + 1}月${d.getDate()}日 周${WEEK[d.getDay()]}`;
}

/** `11月2日 10:29`, with the year when it is not this year. */
export function formatMoment(ms: number, now: Date): string {
  const d = new Date(ms);
  const year = d.getFullYear() === now.getFullYear() ? "" : `${d.getFullYear()}年`;
  return `${year}${d.getMonth() + 1}月${d.getDate()}日 ${isoTime(d)}`;
}

/** `刚刚`, `3 分钟前`, `2 小时前`, `5 天前`. */
export function ago(ms: number, now: Date): string {
  const s = Math.max(0, now.getTime() - ms) / 1000;
  if (s < 60) return "刚刚";
  if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
  if (s < 86_400) return `${Math.floor(s / 3600)} 小时前`;
  return `${Math.floor(s / 86_400)} 天前`;
}

/** The reminders offered for a task due at a time, in minutes before it. */
export const REMINDERS = [0, 5, 15, 30, 60, 120, 1440, 2880, 10_080];

/** And on the due date or days before it, for a task due on a date alone; as menus list them, from the
 * due date back. */
export const DAY_REMINDERS: RemindOn[] = [
  { days: 0, time: "09:00" },
  { days: 1, time: "20:00" },
  { days: 1, time: "09:00" },
  { days: 2, time: "09:00" },
  { days: 7, time: "09:00" },
];

/** The furthest ahead a reminder by the day may be, as the server has it. */
export const MAX_REMIND_DAYS = 30;

const minutesOf = (time: string) => {
  const [h, m] = time.split(":").map(Number);
  return h * 60 + m;
};

/** `reminders` of a task due at `time`, if any, as the server keeps them: those before the time only
 * with one, each once, the first to come first, one before the time ahead of one by the day at the
 * same moment. */
export function inOrder(reminders: Remind[], time: string | undefined): Remind[] {
  // Minutes from the start of the due date.
  const before = time === undefined ? [] : reminders.flatMap((r) => ("before" in r ? [[minutesOf(time) - r.before, r] as const] : []));
  const on = reminders.flatMap((r) => ("days" in r ? [[minutesOf(r.time) - r.days * 1440, r] as const] : []));
  const unique = new Map([...before, ...on].map(([at, r]) => [JSON.stringify(r), [at, r] as const]));
  return [...unique.values()].sort(([a], [b]) => a - b).map(([, r]) => r);
}

/** When reminder `r` of `due` comes. */
export function remindAt(due: Due, r: Remind): Date {
  if ("before" in r) return new Date(deadline(due).getTime() - r.before * 60_000);
  const d = day(due.date);
  const [h, m] = r.time.split(":").map(Number);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - r.days, h, m);
}

/** The furthest ahead a reminder may be, as the server has it: 30 days. */
export const MAX_REMIND = 30 * 1440;

export type RemindUnit = "minute" | "hour" | "day";

export const REMIND_UNITS: Record<RemindUnit, { minutes: number; name: string }> = {
  minute: { minutes: 1, name: "分钟" },
  hour: { minutes: 60, name: "小时" },
  day: { minutes: 1440, name: "天" },
};

/** In whole numbers only: 90 minutes are not 1.5 hours. */
export function remindIn(minutes: number): [number, RemindUnit] {
  return minutes % 1440 === 0 ? [minutes / 1440, "day"] : minutes % 60 === 0 ? [minutes / 60, "hour"] : [minutes, "minute"];
}

/** `截止时`, `提前 90 分钟`, `提前 3 小时`, `提前 2 天`; `当天 09:00`, `前一天 20:00`, `提前 3 天 09:00`. */
export function remindLabel(r: Remind): string {
  if ("days" in r) return r.days === 0 ? `当天 ${r.time}` : r.days === 1 ? `前一天 ${r.time}` : `提前 ${r.days} 天 ${r.time}`;
  if (r.before === 0) return "截止时";
  const [n, unit] = remindIn(r.before);
  return `提前 ${n} ${REMIND_UNITS[unit].name}`;
}

/** All of a due date's reminders, in the order they come: `前一天 20:00、当天 09:00`; `不提醒` for none. */
export const remindersLabel = (reminders: Remind[] | undefined) => (reminders?.length ? reminders.map(remindLabel).join("、") : "不提醒");
