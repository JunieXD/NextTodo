// A twin of `crates/core/src/repeat.rs`; both are checked against `fixtures/repeat.json`.

import type { Repeat } from "../api/types/Repeat";

/** Midnight UTC of a day, years before 100 included (`Date.UTC` takes them for 19xx). */
function utc(y: number, m: number, d: number): Date {
  const t = new Date(0);
  t.setUTCFullYear(y, m - 1, d);
  return t;
}

const daysIn = (y: number, m: number) => utc(y, m + 1, 0).getUTCDate();
const pad = (n: number, width = 2) => String(n).padStart(width, "0");

/** Monday to Friday, as `Repeat.days` has them. */
export const WORKDAYS = [1, 2, 3, 4, 5];

/** The due date after `due`, the task done `today` (both `YYYY-MM-DD`): a step on from the start,
 * or as many as it takes to get past today. Weekly on some days, a step is to the next of them: later
 * that week, or in the first week `every` weeks on. Null past the year 9999, which dates don't go
 * beyond. */
export function nextDue(r: Repeat, due: string, today: string): string | null {
  const [y, m, d] = (r.from === "due" ? due : today).split("-").map(Number);
  // The end of a month stays the end of the month, the 28th or the 31st.
  const monthEnd = d === daysIn(y, m);
  const step = (k: number) => {
    if (r.unit !== "month") return utc(y, m, d + k * r.every * (r.unit === "week" ? 7 : 1));
    const months = y * 12 + m - 1 + k * r.every;
    const [ny, nm] = [Math.floor(months / 12), (months % 12) + 1];
    const last = daysIn(ny, nm);
    return utc(ny, nm, monthEnd ? last : Math.min(d, last));
  };
  const [ty, tm, td] = today.split("-").map(Number);
  const past = utc(ty, tm, td).getTime();
  const stepped = () => {
    let k = 1;
    while (step(k).getTime() <= past) k++;
    return step(k);
  };
  const onDays = (days: number[]) => {
    const after = Math.max(past, utc(y, m, d).getTime());
    const monday = d - ((utc(y, m, d).getUTCDay() + 6) % 7);
    for (let w = 0; ; w += r.every)
      for (let i = 0; i < 7; i++) {
        const day = utc(y, m, monday + 7 * w + i);
        if (days.includes(i + 1) && day.getTime() > after) return day;
      }
  };
  const next = r.unit === "week" && r.days ? onDays(r.days) : stepped();
  const year = next.getUTCFullYear();
  return year > 9999 ? null : `${pad(year, 4)}-${pad(next.getUTCMonth() + 1)}-${pad(next.getUTCDate())}`;
}

/** `每天`, `每 2 周`, `每 3 个月`, `每个工作日`, `每 2 周的周二、四`, as the command line has them too. */
export function repeatLabel(r: Repeat): string {
  if (r.days) {
    if (r.every === 1 && r.days.join() === WORKDAYS.join()) return "每个工作日";
    const names = r.days.map((d) => "一二三四五六日"[d - 1]).join("、");
    return r.every > 1 ? `每 ${r.every} 周的周${names}` : `每周${names}`;
  }
  const unit = { day: "天", week: "周", month: "个月" }[r.unit];
  if (r.every > 1) return `每 ${r.every} ${unit}`;
  return r.unit === "month" ? "每月" : `每${unit}`;
}
