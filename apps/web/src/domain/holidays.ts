// Festivals, solar terms, and the days off and made-up working days the State Council sets around
// public holidays (docs/design/repeat.md#节日和放假): a twin of `crates/core/src/holidays.rs`, reading the same
// tables; both are checked against `fixtures/holidays.json`.

import arrangements from "../../../../crates/core/src/holidays.json";
import terms from "../../../../crates/core/src/terms.json";
import { lunarDate, lunarLabel } from "./lunar";

/** A day the State Council has set: off, or worked to make up for one. */
export type Holiday = "off" | "work";

/** The solar terms in a year, two a month. */
const TERM_NAMES = "小寒 大寒 立春 雨水 惊蛰 春分 清明 谷雨 立夏 小满 芒种 夏至 小暑 大暑 立秋 处暑 白露 秋分 寒露 霜降 立冬 小雪 大雪 冬至".split(" ");

const LUNAR_FESTIVALS: Record<string, string> = {
  "1-1": "春节",
  "1-15": "元宵节",
  "5-5": "端午节",
  "7-7": "七夕",
  "8-15": "中秋节",
  "9-9": "重阳节",
  "12-8": "腊八节",
};

const SOLAR_FESTIVALS: Record<string, string> = {
  "01-01": "元旦",
  "03-08": "妇女节",
  "05-01": "劳动节",
  "06-01": "儿童节",
  "09-10": "教师节",
  "10-01": "国庆节",
};

/** The day after `date`, `YYYY-MM-DD`; not through `Date.UTC`, which takes years before 100 for 19xx. */
function dayAfter(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  const t = new Date(0);
  t.setUTCFullYear(y, m - 1, d + 1);
  return t.toISOString().slice(0, 10);
}

/** The festival on `date`; of two, the lunar one, which moves from year to year. */
export function festival(date: string): string | undefined {
  const l = lunarDate(date);
  const next = lunarDate(dayAfter(date));
  return (
    (l && !l.leap ? LUNAR_FESTIVALS[`${l.month}-${l.day}`] : undefined) ??
    (next && !next.leap && next.month === 1 && next.day === 1 ? "除夕" : undefined) ??
    SOLAR_FESTIVALS[date.slice(5)]
  );
}

/** The solar term on `date`, 1900–2100. */
export function solarTerm(date: string): string | undefined {
  const [y, m, d] = date.split("-").map(Number);
  const days = terms[y - 1900];
  if (!days) return undefined;
  const i = [2 * m - 2, 2 * m - 1].find((i) => Number(days.slice(2 * i, 2 * i + 2)) === d);
  return i === undefined ? undefined : TERM_NAMES[i];
}

/** Whether `date` is a day off or a made-up working day, as the State Council has set it. */
export function holiday(date: string): Holiday | undefined {
  for (const a of arrangements) {
    if (a.from <= date && date <= a.to) return "off";
    if (a.work.includes(date)) return "work";
  }
  return undefined;
}

/** What a calendar writes under day `date`: its festival or solar term, as a `note`, or else its lunar
 * day (on the 1st, its month); nothing outside the tables. */
export function dayLabel(date: string): { text: string; note: boolean } {
  const note = festival(date) ?? solarTerm(date);
  return note ? { text: note, note: true } : { text: lunarLabel(date), note: false };
}
