// The calendar page (docs/ui/calendar.md): a month, a week or a year, what is due and done on each day,
// and the day picked listed under it.

import { ChevronLeft, ChevronRight, LetterText } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { useLocation, useSearchParams } from "wouter";
import { completionKey } from "../domain/apply";
import { type DoneOn, type DueOn, agenda, doneByDay, droppedByDay, dueBetween, heat, lateness, overdue } from "../domain/calendar";
import { day, dueKind, formatToday, isoDate, monthGrid } from "../domain/dates";
import type { Task } from "../api/types/Task";
import { lunarDate, lunarText } from "../domain/lunar";
import { useKept } from "../kept";
import { useNextFolders } from "../next-folders";
import { useView } from "../session";
import { Seg } from "../ui/controls";
import { Tip } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { useTaskActions } from "./actions";
import { DayNote, DayNotes, daySpoken, useHolidayMarks } from "./daynote";
import { useMarks } from "./marks";
import { MonthTitles } from "./monthtitles";
import { ARROWS, WEEKDAYS } from "./due";
import { useSelection } from "./nav";
import { Occurrence, TaskRow } from "./rows";
import { TimeGrid } from "./timegrid";

const VIEWS = ["month", "week", "year", "time"] as const;
export type CalendarView = (typeof VIEWS)[number];

const VIEW_KEY = "nexttodo.calendarView";
const GRID_KEY = "nexttodo.timeGrid";
const TITLES_KEY = "nexttodo.monthTitles";

/** Whether the calendar offers the time grid, set on this device in the settings; on unless turned off. */
export function useTimeGridOn(): [boolean, (on: boolean) => void] {
  const [on, keep] = useKept(GRID_KEY);
  return [on !== "off", (v) => keep(v ? "on" : "off")];
}

/** Whether the month shows the titles of what is on each day, rather than dots, as picked on this device. */
function useMonthTitles(): [boolean, (on: boolean) => void] {
  const [on, keep] = useKept(TITLES_KEY);
  return [on === "on", (v) => keep(v ? "on" : "off")];
}

/** Which of a month, a week, a year or the time grid the calendar shows, remembered on this device
 * under `key`, the panel's apart; the week once the time grid is turned off. */
export function useCalendarView(key = VIEW_KEY): [CalendarView, (v: CalendarView) => void] {
  const [view, keep] = useKept(key);
  const [grid] = useTimeGridOn();
  return [view === "time" && !grid ? "week" : ((view as CalendarView | null) ?? "month"), keep];
}

export function CalendarViewSeg() {
  const [view, setView] = useCalendarView();
  const [grid] = useTimeGridOn();
  return (
    <Seg<CalendarView>
      label="日历的视图"
      items={[
        ["month", "月"],
        ["week", "周"],
        ["year", "年"],
        ...(grid ? [["time", "时间轴"] satisfies [CalendarView, string]] : []),
      ]}
      value={view}
      onChange={setView}
    />
  );
}

/** The day picked in the calendar and, in the time grid, the half hour on it picked to add to: gone once
 * the calendar shows another `view`. */
export function useCalendarPick(now: Date, view: CalendarView) {
  const [picked, setPicked] = useState(() => isoDate(now));
  const [slot, setSlot] = useState<{ date: string; time: string } | null>(null);
  if (view !== "time" && slot) setSlot(null);
  return {
    picked,
    // Another day picked, the half hour goes.
    at: slot?.date === picked ? slot.time : null,
    pick(date: string, time?: string) {
      setPicked(date);
      setSlot(time === undefined ? null : { date, time });
    },
  };
}

export type CalendarPick = ReturnType<typeof useCalendarPick>;

/** Day `date` moved `by` months, weeks or years, as the view turns: the same day of the month, or the
 * last one where there is none. */
function turned(date: string, view: CalendarView, by: number): string {
  const d = day(date);
  if (view === "week" || view === "time") return isoDate(new Date(d.getFullYear(), d.getMonth(), d.getDate() + 7 * by));
  const [y, m] = view === "year" ? [d.getFullYear() + by, d.getMonth()] : [d.getFullYear(), d.getMonth() + by];
  const last = new Date(y, m + 1, 0).getDate();
  return isoDate(new Date(y, m, Math.min(d.getDate(), last)));
}

/** The calendar in `view`, which the year turns into the month of a day picked in it. */
export function CalendarBody({ now, view, onView, pick: { picked, at, pick }, compact = false }: { now: Date; view: CalendarView; onView(v: CalendarView): void; pick: CalendarPick; compact?: boolean }) {
  const { board } = useView();
  // A view and a day in it, as the “时间轴” widget and the panel link to them (`?view=time&on=2026-11-26`):
  // taken once, the link then put away.
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const asked = params.get("view");
  const linked = VIEWS.find((v) => v === asked);
  const on = params.get("on");
  const [timeGrid] = useTimeGridOn();
  const [marks] = useHolidayMarks();
  const [titled, setTitled] = useMonthTitles();
  const compactMonth = view === "month" && compact;
  const titles = view === "month" && titled && !compactMonth;
  useEffect(() => {
    if (asked === null) return;
    if (linked) {
      // With the time grid turned off, the week it would show.
      onView(linked === "time" && !timeGrid ? "week" : linked);
      // A day there is, from a link typed in too.
      if (on && /^\d{4}-\d{2}-\d{2}$/.test(on) && isoDate(day(on)) === on) pick(on);
    }
    navigate(location, { replace: true });
  });
  const setPicked = (date: string) => pick(date);
  const today = isoDate(now);
  const shown = day(picked);
  const days =
    view === "week" || view === "time"
      ? Array.from({ length: 7 }, (_, i) => new Date(shown.getFullYear(), shown.getMonth(), shown.getDate() - ((shown.getDay() + 6) % 7) + i))
      : view === "month"
        ? monthGrid(shown.getFullYear(), shown.getMonth())
        : [];
  const isoDays = days.map(isoDate);
  // The year shows only what was done; the time grid finds what it shows itself.
  const due = view === "year" || view === "time" ? new Map<string, DueOn[]>() : dueBetween(board, isoDays[0], isoDays[isoDays.length - 1]);
  const done = view === "time" ? new Map<string, DoneOn[]>() : doneByDay(board);
  const dropped = view === "year" || view === "time" ? new Map<string, Task[]>() : droppedByDay(board);
  const turn = (by: number) => setPicked(turned(picked, view, by));
  const current = view === "year" ? picked.slice(0, 4) === today.slice(0, 4) : view !== "month" ? isoDays.includes(today) : picked.slice(0, 7) === today.slice(0, 7);
  // The arrow keys move the day picked, which takes the focus, to another month, week or year too.
  const grid = useRef<HTMLDivElement>(null);
  const focus = (date: string) => grid.current!.querySelector<HTMLElement>(`[data-date="${date}"]`)!.focus();
  const move = (e: React.KeyboardEvent, from: Date) => {
    const by = ARROWS.get(e.key);
    if (by === undefined) return;
    e.preventDefault();
    const to = isoDate(new Date(from.getFullYear(), from.getMonth(), from.getDate() + by));
    flushSync(() => setPicked(to));
    focus(to);
  };
  // Swiped sideways across the days, the view turns as with the arrows, and what it began on takes no
  // click from it; a browser may send none, so a key's click (`detail` 0) still counts. Not in what the
  // days open, a panel or a menu, whose events come by React's tree.
  const swipe = useRef<{ x: number; y: number } | null>(null);
  const swept = useRef(false);
  const swiped = (e: React.PointerEvent) => {
    const from = swipe.current;
    swipe.current = null;
    if (!from || e.pointerType === "mouse") return;
    const [dx, dy] = [e.clientX - from.x, e.clientY - from.y];
    if (Math.abs(dx) > 48 && Math.abs(dx) > 2 * Math.abs(dy)) {
      swept.current = true;
      turn(dx < 0 ? 1 : -1);
    }
  };
  // Another view, in the same scroll as the one before, which may have been far down (the time grid
  // opens at an hour), starts with its month and arrows in sight.
  const bar = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (view !== "time") bar.current!.scrollIntoView({ block: "nearest" });
  }, [view]);
  return (
    <div className={`cal-page${view === "time" ? " time" : titles ? " titles" : compactMonth ? " compact-month" : ""}`}>
      <div ref={bar} className="cal-bar">
        <span aria-live="polite">{view === "year" ? `${shown.getFullYear()}年` : `${shown.getFullYear()}年${shown.getMonth() + 1}月`}</span>
        {view === "month" && !compactMonth && (
          <Tip label="显示标题">
            <button type="button" className="icon-btn sm" aria-pressed={titled} onClick={() => setTitled(!titled)}>
              <Icon of={LetterText} size={18} />
            </button>
          </Tip>
        )}
        {!current && (
          <button type="button" className="text-btn" onClick={() => setPicked(today)}>
            今天
          </button>
        )}
        <Tip label={view === "year" ? "上一年" : view === "month" ? "上个月" : "上一周"}>
          <button type="button" className="icon-btn sm" onClick={() => turn(-1)}>
            <Icon of={ChevronLeft} size={18} />
          </button>
        </Tip>
        <Tip label={view === "year" ? "下一年" : view === "month" ? "下个月" : "下一周"}>
          <button type="button" className="icon-btn sm" onClick={() => turn(1)}>
            <Icon of={ChevronRight} size={18} />
          </button>
        </Tip>
      </div>
      <div
        ref={grid}
        className="cal-swipe"
        onPointerDown={(e) => {
          swept.current = false;
          swipe.current = grid.current!.contains(e.target as Node) ? { x: e.clientX, y: e.clientY } : null;
        }}
        onPointerUp={swiped}
        onPointerCancel={() => (swipe.current = null)}
        onClickCapture={(e) => {
          if (swept.current && e.detail > 0) e.stopPropagation();
          swept.current = false;
        }}
      >
        {view === "year" ? (
          <Year
            year={shown.getFullYear()}
            today={today}
            picked={picked}
            done={done}
            onMove={move}
            onPick={(d) => {
              flushSync(() => {
                setPicked(d);
                onView("month");
              });
              focus(d);
            }}
          />
        ) : view === "time" ? (
          <TimeGrid days={isoDays} today={today} picked={picked} at={at} now={now} onPick={pick} />
        ) : titles ? (
          <>
            <div className="cal-week" aria-hidden>
              {[...WEEKDAYS].map((w) => (
                <span key={w}>{w}</span>
              ))}
            </div>
            <MonthTitles days={isoDays} month={shown.getMonth()} today={today} picked={picked} now={now} onPick={setPicked} onMove={(e, d) => move(e, day(d))} />
          </>
        ) : (
          <>
            <div className="cal-week" aria-hidden>
              {[...WEEKDAYS].map((w) => (
                <span key={w}>{w}</span>
              ))}
            </div>
            <div className="cal-grid">
              {days.map((d, i) => {
                const iso = isoDays[i];
                const dots = due.get(iso) ?? [];
                const n = done.get(iso)?.length ?? 0;
                // The other months' days, in their faint colour, aren't tinted: on a tint it would be hard to read.
                const out = view === "month" && d.getMonth() !== shown.getMonth();
                return (
                  <button
                    key={iso}
                    type="button"
                    data-date={iso}
                    className={out ? "out" : undefined}
                    tabIndex={iso === picked ? 0 : -1}
                    aria-pressed={iso === picked}
                    aria-current={iso === today ? "date" : undefined}
                    aria-label={`${d.getMonth() + 1}月${d.getDate()}日${daySpoken(iso, marks)}，${dots.length} 项截止${n ? `，完成 ${n} 项` : ""}`}
                    onClick={() => setPicked(iso)}
                    onKeyDown={(e) => move(e, d)}
                  >
                    <span className={n && !out ? `heat-${heat(n)}` : undefined}>
                      <span className="cal-num">{d.getDate()}</span>
                      <DayNote date={iso} marks={marks} />
                      <span className="cal-dots">
                        {dots.slice(0, 3).map((o, j) => (
                          <i key={j} className={!o.later && dueKind(o.task.due!, now) === "overdue" ? "overdue" : undefined} />
                        ))}
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          </>
        )}
      </div>
      {(view === "month" || view === "week") && <DayList date={picked} now={now} overdue={picked === today ? overdue(board, today) : []} due={due.get(picked) ?? []} dropped={dropped.get(picked) ?? []} done={done.get(picked) ?? []} />}
    </div>
  );
}

export function AgendaBody({ now }: { now: Date }) {
  const { board } = useView();
  const folders = useNextFolders(board);
  const actions = useTaskActions();
  const { linger, leaving } = useMarks();
  const { overdue, days } = agenda(board, 7);
  const listed = (task: Task) => folders.some((f) => board.inSubtree(task.folder_id, f));
  const late = overdue.filter(listed);
  const today = day(board.now.today);
  const dateIn = (n: number) => isoDate(new Date(today.getFullYear(), today.getMonth(), today.getDate() + n));
  const tomorrow = dateIn(1);
  const last = dateIn(6);
  const groups = new Map(days.map(([date, due]) => [date, due.filter((o) => listed(o.task))]));
  for (const id of linger.keys()) {
    const task = board.task(id);
    if (!task?.due || task.done_at === null || !listed(task)) continue;
    const { date } = task.due;
    if (date < board.now.today) late.push(task);
    else if (date <= last) groups.set(date, [...(groups.get(date) ?? []), { task, later: false }]);
  }
  const shown = [...groups].filter(([, due]) => due.length > 0).sort(([a], [b]) => a.localeCompare(b));
  if (!late.length && !shown.length) return <p className="empty">这 7 天没有截止的事项</p>;
  const row = (task: Task) => <TaskRow key={task.id} task={task} board={board} now={now} path dueTimeOnly leaving={task.done_at !== null && leaving.has(task.id)} selected={false} actions={actions} />;
  return (
    <>
      {late.length > 0 && (
        <section>
          <h2 className="group-title overdue">已逾期</h2>
          <div className="group">{board.byDue(late).map(row)}</div>
        </section>
      )}
      {shown.map(([date, due]) => {
        const d = day(date);
        const heading = date === board.now.today ? "今天" : date === tomorrow ? "明天" : `周${WEEKDAYS[(d.getDay() + 6) % 7]} ${d.getMonth() + 1}月${d.getDate()}日`;
        const order = new Map(board.sort(due.map(({ task }) => task)).map((task, i) => [task.id, i]));
        const time = (o: DueOn) => o.task.due?.time ?? "24:00";
        due.sort((a, b) => time(a).localeCompare(time(b)) || order.get(a.task.id)! - order.get(b.task.id)!);
        return (
          <section key={date}>
            <h2 className="group-title">{heading}</h2>
            <div className="group">
              {due.map(({ task, later }) =>
                later ? <Occurrence key={`later ${task.id}`} task={task} meta="以后的一次" time={task.due?.time} onOpen={() => actions.open(task)} /> : row(task),
              )}
            </div>
          </section>
        );
      })}
    </>
  );
}

/** A year of days, tinted by what was done on them; a day picked opens its month. Tab comes to the
 * day picked, which the arrow keys move. */
function Year(p: {
  year: number;
  today: string;
  picked: string;
  done: Map<string, DoneOn[]>;
  onMove(e: React.KeyboardEvent, from: Date): void;
  onPick(day: string): void;
}) {
  const { year, today, picked, done, onMove, onPick } = p;
  return (
    <div className="cal-year">
      {Array.from({ length: 12 }, (_, m) => {
        const first = new Date(year, m, 1);
        const length = new Date(year, m + 1, 0).getDate();
        return (
          <section key={m} className="cal-mini">
            <h3>{m + 1}月</h3>
            <div style={{ "--skip": (first.getDay() + 6) % 7 }}>
              {Array.from({ length }, (_, i) => {
                const iso = isoDate(new Date(year, m, i + 1));
                const n = done.get(iso)?.length ?? 0;
                return (
                  <button
                    key={iso}
                    type="button"
                    data-date={iso}
                    className={n ? `heat-${heat(n)}` : undefined}
                    tabIndex={iso === picked ? 0 : -1}
                    aria-current={iso === today ? "date" : undefined}
                    aria-label={`${m + 1}月${i + 1}日${n ? `，完成 ${n} 项` : ""}`}
                    title={`${m + 1}月${i + 1}日${n ? ` · 完成 ${n} 项` : ""}`}
                    onClick={() => onPick(iso)}
                    onKeyDown={(e) => onMove(e, new Date(year, m, i + 1))}
                  />
                );
              })}
            </div>
          </section>
        );
      })}
    </div>
  );
}

/** What is due and done on `date`, and for today, what is `overdue`; given up, what was `dropped`
 * though due then. */
function DayList({ date, now, overdue, due, dropped, done }: { date: string; now: Date; overdue: Task[]; due: DueOn[]; dropped: Task[]; done: DoneOn[] }) {
  const { board } = useView();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const lunar = lunarDate(date);
  const [marks] = useHolidayMarks();
  return (
    <section className="cal-day">
      <h2 className="cal-date">
        {formatToday(day(date))}
        {lunar && <span className="cal-lunar">农历{lunarText(lunar)}</span>}
        <DayNotes date={date} marks={marks} />
      </h2>
      {!overdue.length && !due.length && !dropped.length && !done.length && <p className="empty">这天没有事项</p>}
      {overdue.length > 0 && (
        <>
          <h3 className="group-title overdue">已逾期</h3>
          <div className="group">
            {overdue.map((task) => (
              <TaskRow key={task.id} task={task} board={board} now={now} path selected={selected === task.id} actions={actions} />
            ))}
          </div>
        </>
      )}
      {due.length + dropped.length > 0 && (
        <>
          <h3 className="group-title">截止</h3>
          <div className="group">
            {due.map(({ task, later }) =>
              later ? (
                <Occurrence key={`later ${task.id}`} task={task} meta="以后的一次" onOpen={() => actions.open(task)} />
              ) : (
                <TaskRow key={task.id} task={task} board={board} now={now} path selected={selected === task.id} actions={actions} />
              ),
            )}
            {dropped.map((task) => (
              <TaskRow key={task.id} task={task} board={board} now={now} path selected={selected === task.id} actions={actions} />
            ))}
          </div>
        </>
      )}
      {done.length > 0 && (
        <>
          <h3 className="group-title">完成</h3>
          <div className="group">
            {done.map(({ task, record }) =>
              record ? (
                <Occurrence key={`record ${completionKey(record)}`} done task={task} meta={`完成一次 · ${lateness(record)}`} onOpen={() => actions.open(task)} />
              ) : (
                <TaskRow key={task.id} task={task} board={board} now={now} path selected={selected === task.id} actions={actions} />
              ),
            )}
          </div>
        </>
      )}
    </section>
  );
}
