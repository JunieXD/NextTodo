// The calendar's month with titles (docs/ui/calendar.md): six weeks of days edge to edge, each with what is on
// it as the time grid's labels, instead of dots and the tint of what was done.

import { useLayoutEffect, useRef, useState } from "react";
import { useView } from "../session";
import { openMain } from "../desktop";
import { cmp } from "../domain/board";
import { fit, weekBlocks } from "../domain/timegrid";
import { useWide } from "../ui/hooks";
import { DayNote, daySpoken, useHolidayMarks } from "./daynote";
import { useTaskActions } from "./actions";
import { DayBlock, keyOf } from "./timegrid";

/** A title's line, `.tg-label`'s with the gap above it. */
const TITLE_LINE = 20;

/** `days`, six weeks, `month` the one of them shown; `picked` outlined, which a tap on a day changes and
 * the arrow keys move from it (`onMove`). On a phone a day is all one tap, its titles too small to aim
 * at; on a computer a title opens its task. With `fill`, in the panel and the desktop widgets, the weeks
 * share the height there is, each day with as many titles as fit, and how many more opens the day in the
 * main window. */
export function MonthTitles({
  days,
  month,
  today,
  picked,
  now,
  fill,
  onPick,
  onMove,
}: {
  days: string[];
  month: number;
  today: string;
  picked: string;
  now: Date;
  fill: boolean;
  onPick(date: string): void;
  onMove(e: React.KeyboardEvent, date: string): void;
}) {
  const { board } = useView();
  const precise = useWide() || fill;
  const actions = useTaskActions();
  const marks = useHolidayMarks();
  const blocks = weekBlocks(board, days, now);
  const grid = useRef<HTMLDivElement>(null);
  // As many lines as there is room for under a day's number, one at least: the days are as high as each
  // other. The first is another one in another month, the grid the same.
  const [fits, setFits] = useState(1);
  useLayoutEffect(() => {
    if (!fill) return;
    const g = grid.current!;
    const watch = new ResizeObserver(() => {
      const day = g.firstElementChild!;
      const under = day.getBoundingClientRect().bottom - day.querySelector(".cal-titled-day")!.getBoundingClientRect().bottom;
      setFits(Math.max(1, Math.floor(under / TITLE_LINE)));
    });
    watch.observe(g);
    return () => watch.disconnect();
  }, [fill]);
  const room = fill ? fits : precise ? 4 : 3;
  return (
    <div ref={grid} className="cal-titles">
      {days.map((d) => {
        // Those with a time first, as the agenda has them.
        const list = [...blocks.get(d)!].sort((a, b) => cmp(a.time ?? "24:00", b.time ?? "24:00"));
        const { shown, merged } = fit(list, room);
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
              precise ? (
                // Tab goes through the picked day's alone, the arrow keys to the others.
                <DayBlock key={keyOf(b)} block={b} now={now} actions={actions} label tabIndex={d === picked ? 0 : -1} />
              ) : (
                <span key={keyOf(b)} className={`tg-label ${b.look}`}>
                  {b.task.title}
                </span>
              ),
            )}
            {merged.length > 0 &&
              (fill ? (
                <button type="button" className="tg-label more" tabIndex={d === picked ? 0 : -1} onClick={() => void openMain(`/calendar?view=month&on=${d}`)}>
                  +{merged.length}
                </button>
              ) : (
                <span className="tg-label more">+{merged.length}</span>
              ))}
          </div>
        );
      })}
    </div>
  );
}
