// Due dates as people read them. Dates and times are the user's local ones, as stored.

import type { Due } from "../api/types/Due";

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

/** `明天开始`, `周五开始`, `11月14日开始`: the day a task starts, written as due dates are. */
export const startLabel = (start: string, now: Date) => `${formatDue({ date: start }, now)}开始`;

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

/** The reminder choices offered, in minutes before the deadline. */
export const REMINDERS = [0, 5, 15, 30, 60, 120, 1440, 2880, 10_080];

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

export function remindLabel(minutes: number | undefined): string {
  if (minutes === undefined) return "不提醒";
  if (minutes === 0) return "截止时";
  const [n, unit] = remindIn(minutes);
  return `提前 ${n} ${REMIND_UNITS[unit].name}`;
}
