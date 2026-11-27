// The calendar's month with titles (docs/ui/calendar.md): six weeks of days edge to edge, each with what is on
// it as the time grid's labels, instead of dots and the tint of what was done.

import { useView } from "../session";
import { cmp } from "../domain/board";
import { fit, weekBlocks } from "../domain/timegrid";
import { useWide } from "../ui/hooks";
import { DayNote, daySpoken, useHolidayMarks } from "./daynote";
import { useTaskActions } from "./actions";
import { DayBlock, keyOf } from "./timegrid";

/** `days`, six weeks, `month` the one of them shown; `picked` outlined, which a tap on a day changes and
 * the arrow keys move from it (`onMove`). On a phone a day is all one tap, its titles too small to aim
 * at; on a computer a title opens its task. */
export function MonthTitles({
  days,
  month,
  today,
  picked,
  now,
  onPick,
  onMove,
}: {
  days: string[];
  month: number;
  today: string;
  picked: string;
  now: Date;
  onPick(date: string): void;
  onMove(e: React.KeyboardEvent, date: string): void;
}) {
  const { board } = useView();
  const wide = useWide();
  const actions = useTaskActions();
  const [marks] = useHolidayMarks();
  const blocks = weekBlocks(board, days, now);
  return (
    <div className="cal-titles">
      {days.map((d) => {
        // Those with a time first, as the agenda has them.
        const list = [...blocks.get(d)!].sort((a, b) => cmp(a.time ?? "24:00", b.time ?? "24:00"));
        const { shown, merged } = fit(list, wide ? 4 : 3);
        const classes = [Number(d.slice(5, 7)) - 1 === month ? "" : "out", d === today ? "today" : "", d === picked ? "picked" : ""].filter(Boolean);
        return (
          <div
            key={d}
            className={["cal-titled", ...classes].join(" ")}
            onClick={(e) => {
              // A title's menu and the picker it opens send their clicks here too, by React's tree.
              if (!e.currentTarget.contains(e.target as Node)) return;
              onPick(d);
              // For the arrow keys, as a tap on a day of the month without titles.
              e.currentTarget.querySelector<HTMLElement>(".cal-titled-day")!.focus();
            }}
          >
            <button
              type="button"
              data-date={d}
              className="cal-titled-day"
              tabIndex={d === picked ? 0 : -1}
              aria-pressed={d === picked}
              aria-current={d === today ? "date" : undefined}
              aria-label={`${Number(d.slice(5, 7))}月${Number(d.slice(8))}日${daySpoken(d, marks)}，${list.length} 项`}
              onKeyDown={(e) => onMove(e, d)}
            >
              <span className="cal-num">{Number(d.slice(8))}</span>
              <DayNote date={d} marks={marks} />
            </button>
            {shown.map((b) =>
              wide ? (
                // Tab goes through the picked day's alone, the arrow keys to the others.
                <DayBlock key={keyOf(b)} block={b} now={now} actions={actions} label tabIndex={d === picked ? 0 : -1} />
              ) : (
                <span key={keyOf(b)} className={`tg-label ${b.look}`}>
                  {b.task.title}
                </span>
              ),
            )}
            {merged.length > 0 && <span className="tg-label more">+{merged.length}</span>}
          </div>
        );
      })}
    </div>
  );
}
