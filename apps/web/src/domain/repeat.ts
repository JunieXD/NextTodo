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

/** The due date after `due`, the task done `today` (both `YYYY-MM-DD`): a step on from the start,
 * or as many as it takes to get past today; null past the year 9999, which dates don't go beyond. */
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
  let k = 1;
  while (step(k).getTime() <= past) k++;
  const next = step(k);
  const year = next.getUTCFullYear();
  return year > 9999 ? null : `${pad(year, 4)}-${pad(next.getUTCMonth() + 1)}-${pad(next.getUTCDate())}`;
}

/** `每天`, `每 2 周`, `每 3 个月`, as the command line has them too. */
export function repeatLabel(r: Repeat): string {
  const unit = { day: "天", week: "周", month: "个月" }[r.unit];
  if (r.every > 1) return `每 ${r.every} ${unit}`;
  return r.unit === "month" ? "每月" : `每${unit}`;
}
