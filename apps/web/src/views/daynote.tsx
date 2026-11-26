// What calendars write with a day (docs/design.md#节日和放假): under its number its festival or solar
// term, or else its lunar day; in a corner 休 or 班 where the State Council has set a day off or a
// made-up working day, unless that is turned off on this device.

import { festival, holiday, dayLabel, solarTerm } from "../domain/holidays";
import { lunarDate, lunarText } from "../domain/lunar";
import { useKept } from "../kept";

const MARKS_KEY = "nexttodo.holidayMarks";

/** Whether calendars mark days off and made-up working days, set on this device; on unless turned off. */
export function useHolidayMarks(): [boolean, (on: boolean) => void] {
  const [on, keep] = useKept(MARKS_KEY);
  return [on !== "off", (v) => keep(v ? "on" : "off")];
}

/** The same, read now, as the widgets' labels are worked out outside the page's components. */
export const holidayMarksOn = () => localStorage.getItem(MARKS_KEY) !== "off";

/** Under a day's number: its festival, solar term or lunar day; with `marks`, 休 or 班 in the corner of
 * the tile, which is positioned. */
export function DayNote({ date, marks }: { date: string; marks: boolean }) {
  const { text, note } = dayLabel(date);
  const h = marks ? holiday(date) : undefined;
  return (
    <>
      <small className={note ? "day-note" : undefined}>{text}</small>
      {h && <i className={`day-mark ${h}`}>{h === "off" ? "休" : "班"}</i>}
    </>
  );
}

/** What a screen reader says after a day's date: `，农历正月初一，春节，放假`. */
export function daySpoken(date: string, marks: boolean): string {
  const l = lunarDate(date);
  const h = marks ? holiday(date) : undefined;
  return [l && `农历${lunarText(l)}`, festival(date), solarTerm(date), h && (h === "off" ? "放假" : "补班")]
    .filter(Boolean)
    .map((s) => `，${s}`)
    .join("");
}

/** After a day's date in a heading: its festival and solar term, and 放假 or 补班. */
export function DayNotes({ date, marks }: { date: string; marks: boolean }) {
  const h = marks ? holiday(date) : undefined;
  const notes = [festival(date), solarTerm(date)].filter(Boolean);
  return (
    <>
      {notes.length > 0 && <span className="cal-lunar day-note">{notes.join(" ")}</span>}
      {h && <span className={`cal-holiday ${h}`}>{h === "off" ? "放假" : "补班"}</span>}
    </>
  );
}
