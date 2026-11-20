// The Chinese lunar calendar (docs/design.md#农历): a twin of `crates/core/src/lunar.rs`, reading the
// same table; both are checked against `fixtures/lunar.json`. Not `Intl`'s, which is a day out at times.

import years from "../../../../crates/core/src/lunar.json";

const DAY = 86_400_000;

/** A lunar month: its first day, as days since 1970-01-01, how many days it has, and which it is. */
interface Entry {
  start: number;
  len: number;
  year: number;
  number: number;
  leap: boolean;
}

/** Every month of the lunar years 1900–2100, in order; `lunar.rs` says how the table has them. */
const MONTHS: Entry[] = years.flatMap((bits, i) => {
  const leap = (bits >> 13) & 0xf;
  let start = Date.UTC(1900 + i, 0, 21) / DAY + (bits >> 17);
  return Array.from({ length: leap ? 13 : 12 }, (_, m) => {
    const [number, isLeap] = !leap || m < leap ? [m + 1, false] : [m, m === leap];
    const len = bits & (1 << m) ? 30 : 29;
    start += len;
    return { start: start - len, len, year: 1900 + i, number, leap: isLeap };
  });
});

/** The month, as its place in the table, day `n` (days since 1970-01-01) is in. */
export function monthOf(n: number): number | undefined {
  let [lo, hi] = [0, MONTHS.length];
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (MONTHS[mid].start <= n) lo = mid + 1;
    else hi = mid;
  }
  const i = lo - 1;
  return i >= 0 && n < MONTHS[i].start + MONTHS[i].len ? i : undefined;
}

/** The month `count` months after month `i`, a leap month counted too; or, `byYear`, the month of the
 * same name `count / 12` years on, never a leap one. */
export function monthOn(i: number, count: number, byYear: boolean): number | undefined {
  if (count === 0) return i;
  if (!byYear) return i + count < MONTHS.length ? i + count : undefined;
  const { year, number } = MONTHS[i];
  const found = MONTHS.findIndex((m) => m.year === year + count / 12 && m.number === number && !m.leap);
  return found < 0 ? undefined : found;
}

/** Day `d`, 1–30, of month `i`, or its last if it has no such day: as days since 1970-01-01. */
export const dayOf = (i: number, d: number) => MONTHS[i].start + Math.min(d, MONTHS[i].len) - 1;

/** A day of the lunar calendar. */
export interface LunarDate {
  /** 1–12. */
  month: number;
  leap: boolean;
  /** 1–30. */
  day: number;
}

/** Day `date` (`YYYY-MM-DD`) in the lunar calendar, if the table has it. */
export function lunarDate(date: string): LunarDate | undefined {
  const [y, m, d] = date.split("-").map(Number);
  // Not `Date.UTC`, which takes years before 100 for 19xx.
  const t = new Date(0);
  t.setUTCFullYear(y, m - 1, d);
  const n = t.getTime() / DAY;
  const i = monthOf(n);
  if (i === undefined) return undefined;
  return { month: MONTHS[i].number, leap: MONTHS[i].leap, day: n - MONTHS[i].start + 1 };
}

/** 正月, 二月 … 十月, 冬月, 腊月. */
export const monthName = (month: number) => ["正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "冬月", "腊月"][month - 1];

/** 初一 … 初十, 十一 … 二十, 廿一 … 廿九, 三十. */
export const dayName = (day: number) =>
  ({ 10: "初十", 20: "二十", 30: "三十" })[day] ?? ["初", "十", "廿"][Math.floor(day / 10)] + "一二三四五六七八九"[(day % 10) - 1];

/** 八月十五, 闰六月初一. */
export const lunarText = (l: LunarDate) => `${l.leap ? "闰" : ""}${monthName(l.month)}${dayName(l.day)}`;

/** What a calendar writes under day `date`: the lunar day, or on the 1st its month; nothing outside the
 * table. */
export function lunarLabel(date: string): string {
  const l = lunarDate(date);
  if (!l) return "";
  return l.day === 1 ? `${l.leap ? "闰" : ""}${monthName(l.month)}` : dayName(l.day);
}
