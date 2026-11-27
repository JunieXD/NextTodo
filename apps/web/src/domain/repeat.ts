// A twin of `crates/core/src/repeat.rs`; both are checked against `fixtures/repeat.json`.

import type { Due } from "../api/types/Due";
import type { Occurrence } from "../api/types/Occurrence";
import type { Repeat } from "../api/types/Repeat";
import type { Task } from "../api/types/Task";
import { formatDue } from "./dates";
import { holiday } from "./holidays";
import { dayName, dayOf, lunarDate, monthName, monthOf, monthOn } from "./lunar";

/** Midnight UTC of a day, years before 100 included (`Date.UTC` takes them for 19xx). */
function utc(y: number, m: number, d: number): Date {
  const t = new Date(0);
  t.setUTCFullYear(y, m - 1, d);
  return t;
}

const DAY = 86_400_000;
const daysIn = (y: number, m: number) => utc(y, m + 1, 0).getUTCDate();
const pad = (n: number, width = 2) => String(n).padStart(width, "0");
const iso = (t: Date) => `${pad(t.getUTCFullYear(), 4)}-${pad(t.getUTCMonth() + 1)}-${pad(t.getUTCDate())}`;

/** Whether `day` is worked as the State Council sets it: Monday to Friday unless it is a day off, or a
 * weekend day worked to make up for one. Past the table, Monday to Friday. */
function legalWorkday(day: Date): boolean {
  const h = holiday(iso(day));
  return h ? h === "work" : day.getUTCDay() % 6 !== 0;
}

/** Monday to Friday, as `Repeat.days` has them. */
export const WORKDAYS = [1, 2, 3, 4, 5];

/** The due date after `due`, the task done `today` (both `YYYY-MM-DD`): a step on from the start,
 * or as many as it takes to reach today, so that the times missed are made up for at once and today's
 * is still to do. On some days of the week or month, a step is to the next
 * of them: later that week or month, or in the first one `every` on. Null when that was the last
 * time: the next is after `until`, past the year 9999, which dates don't go beyond, or past the lunar
 * calendar's table for a repeat by it. */
export function nextDue(r: Repeat, due: string, today: string): string | null {
  const [y, m, d] = (r.from === "due" ? due : today).split("-").map(Number);
  // The day `day` of the month `k` months on, or its last day if it has none.
  const inMonth = (k: number, day: number) => {
    const months = y * 12 + m - 1 + k;
    const [ny, nm] = [Math.floor(months / 12), (months % 12) + 1];
    return utc(ny, nm, Math.min(day, daysIn(ny, nm)));
  };
  const step = (k: number) =>
    r.unit === "month"
      ? // The end of a month stays the end of the month, the 28th or the 31st.
        inMonth(k * r.every, d === daysIn(y, m) ? 31 : d)
      : utc(y, m, d + k * r.every * (r.unit === "week" ? 7 : 1));
  const [ty, tm, td] = today.split("-").map(Number);
  const todayTime = utc(ty, tm, td).getTime();
  const startTime = utc(y, m, d).getTime();
  // After the start, and today or later.
  const counts = (day: Date) => day.getTime() > startTime && day.getTime() >= todayTime;
  const stepped = () => {
    let k = 1;
    while (step(k).getTime() < todayTime) k++;
    return step(k);
  };
  const onWeekdays = (days: number[]) => {
    const monday = d - ((utc(y, m, d).getUTCDay() + 6) % 7);
    for (let w = 0; ; w += r.every)
      for (let i = 0; i < 7; i++) {
        const day = utc(y, m, monday + 7 * w + i);
        if (days.includes(i + 1) && counts(day)) return day;
      }
  };
  const onMonthdays = (days: number[]) => {
    for (let k = 0; ; k += r.every)
      for (let i = 1; i <= 31; i++) {
        const day = inMonth(k, i);
        if (days.includes(i) && counts(day)) return day;
      }
  };
  const onLunarDays = (days: number[]) => {
    const month = monthOf(utc(y, m, d).getTime() / DAY);
    for (let k = 0; month !== undefined; k++) {
      const at = monthOn(month, k * r.every, r.every % 12 === 0);
      if (at === undefined) return undefined;
      for (let i = 1; i <= 30; i++) {
        const day = new Date(dayOf(at, i) * DAY);
        if (days.includes(i) && counts(day)) return day;
      }
    }
  };
  const onLegalWorkdays = () => {
    for (let i = 1; ; i++) {
      const day = utc(y, m, d + i);
      if (counts(day) && legalWorkday(day)) return day;
    }
  };
  const next = r.legal
    ? onLegalWorkdays()
    : !r.days
      ? stepped()
      : r.lunar
        ? onLunarDays(r.days)
        : r.unit === "week"
          ? onWeekdays(r.days)
          : onMonthdays(r.days);
  if (!next) return null;
  const date = iso(next);
  return next.getUTCFullYear() > 9999 || (r.until && date > r.until) ? null : date;
}

/** Whether a repeating task was done `today` and is next due after it: catching up on a time missed
 * leaves it due today still, today's time to do (docs/design/repeat.md#重复). */
export const doneToday = (t: Task, today: string) => t.completions?.last === today && (t.due?.date ?? "") > today;

/** Whether a task due `due`, done `today`, is done for good: it repeats, and this was the last time. */
export const lastTime = (due: Due, today: string) => due.repeat !== undefined && nextDue(due.repeat, due.date, today) === null;

/** Where skipping the time due `due` moves it `today`, never back: skipped early, one counted from the
 * day done is counted from its due date. Null when that was the last time. */
export const skipTo = (due: Due, today: string) => (due.repeat ? nextDue(due.repeat, due.date, due.date > today ? due.date : today) : null);

/** Whether a task can skip to its next time `today`: it repeats, is to do, and this isn't the last time. */
export const skippable = (t: Task, today: string) => t.done_at === null && t.due !== null && skipTo(t.due, today) !== null;

/** The time a task is due now, should it repeat, as `Task::occurrence` in Rust has it. */
export const occurrence = (t: Task): Occurrence | undefined => (t.due?.repeat ? { due: t.due.date, completions: t.completions?.count ?? 0 } : undefined);

/** The date `days` days after `date`, but no later than 9999-12-31: dates go no further. */
export function plus(date: string, days: number): string {
  const [y, m, d] = date.split("-").map(Number);
  const t = utc(y, m, d + days);
  return t.getUTCFullYear() > 9999 ? "9999-12-31" : iso(t);
}

/** The moment `date` begins in UTC, to count days by alike in every time zone. */
function utcDay(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return utc(y, m, d).getTime();
}

/** How many days `date` is before `due`; below 0 for one after it. */
export const daysBefore = (date: string, due: string) => Math.round((utcDay(due) - utcDay(date)) / 864e5);

/** `date` moved as many days as `from` moved to `to`: a start date kept as far before its due date. */
export function shifted(date: string, from: string, to: string): string {
  return iso(new Date(utcDay(date) + utcDay(to) - utcDay(from)));
}

/** `每天`, `每 2 周`, `每 3 个月`, `每个工作日`, `每个法定工作日`, `每 2 周的周二、四`, `每月 1、15 号`, `每年`; by the lunar
 * calendar `农历每月初一、十五`, `每年农历八月十五`, its month that of the task `due`. As the command
 * line has them too. */
export function repeatLabel(r: Repeat, due: string): string {
  if (r.days && r.lunar) {
    const days = r.days.map(dayName).join("、");
    // Picked past the table in the editor, which says so before it is saved.
    const l = lunarDate(due);
    const month = l ? monthName(l.month) : "";
    if (r.every === 1) return `农历每月${days}`;
    if (r.every === 12) return `每年农历${month}${days}`;
    return r.every % 12 === 0 ? `每 ${r.every / 12} 年的农历${month}${days}` : `农历每 ${r.every} 个月的${days}`;
  }
  if (r.legal) return "每个法定工作日";
  if (r.days && r.unit === "week") {
    if (r.every === 1 && r.days.join() === WORKDAYS.join()) return "每个工作日";
    const names = r.days.map((d) => "一二三四五六日"[d - 1]).join("、");
    return r.every > 1 ? `每 ${r.every} 周的周${names}` : `每周${names}`;
  }
  if (r.days) return r.every > 1 ? `每 ${r.every} 个月的 ${r.days.join("、")} 号` : `每月 ${r.days.join("、")} 号`;
  if (r.unit === "month" && r.every % 12 === 0) return r.every === 12 ? "每年" : `每 ${r.every / 12} 年`;
  const unit = { day: "天", week: "周", month: "个月" }[r.unit];
  if (r.every > 1) return `每 ${r.every} ${unit}`;
  return r.unit === "month" ? "每月" : `每${unit}`;
}

/** `每天，到12月19日`: how a task due `due` repeats, and until when if it ends. */
export const repeatText = (r: Repeat, due: string, now: Date) =>
  r.until ? `${repeatLabel(r, due)}，到${formatDue({ date: r.until }, now)}` : repeatLabel(r, due);
