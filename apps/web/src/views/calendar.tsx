// The calendar page (docs/ui.md#日历): a month, a week or a year, what is due and done on each day,
// and the day picked listed under it.

import { Check as CheckIcon, ChevronLeft, ChevronRight } from "lucide-react";
import { useRef, useSyncExternalStore } from "react";
import { flushSync } from "react-dom";
import { completionKey } from "../domain/apply";
import { type DoneOn, type DueOn, doneByDay, dueBetween, heat, lateness, overdue } from "../domain/calendar";
import { day, dueKind, formatToday, isoDate, monthGrid } from "../domain/dates";
import type { Task } from "../api/types/Task";
import { lunarDate, lunarLabel, lunarText } from "../domain/lunar";
import { useView } from "../session";
import { Seg } from "../ui/controls";
import { Tip } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { useTaskActions } from "./actions";
import { ARROWS, WEEKDAYS } from "./due";
import { useSelection } from "./nav";
import { PathMeta, TaskRow } from "./rows";

export type CalendarView = "month" | "week" | "year";

const VIEW_KEY = "nexttodo.calendarView";
const listeners = new Set<() => void>();

/** Which of a month, a week or a year the calendar shows, remembered on this device. */
export function useCalendarView(): [CalendarView, (v: CalendarView) => void] {
  const view = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => (localStorage.getItem(VIEW_KEY) as CalendarView | null) ?? "month",
  );
  const setView = (v: CalendarView) => {
    localStorage.setItem(VIEW_KEY, v);
    for (const l of listeners) l();
  };
  return [view, setView];
}

export function CalendarViewSeg() {
  const [view, setView] = useCalendarView();
  return (
    <Seg<CalendarView>
      label="日历的视图"
      items={[
        ["month", "月"],
        ["week", "周"],
        ["year", "年"],
      ]}
      value={view}
      onChange={setView}
    />
  );
}

/** Day `date` moved `by` months, weeks or years, as the view turns: the same day of the month, or the
 * last one where there is none. */
function turned(date: string, view: CalendarView, by: number): string {
  const d = day(date);
  if (view === "week") return isoDate(new Date(d.getFullYear(), d.getMonth(), d.getDate() + 7 * by));
  const [y, m] = view === "year" ? [d.getFullYear() + by, d.getMonth()] : [d.getFullYear(), d.getMonth() + by];
  const last = new Date(y, m + 1, 0).getDate();
  return isoDate(new Date(y, m, Math.min(d.getDate(), last)));
}

export function CalendarBody({ now, picked, setPicked }: { now: Date; picked: string; setPicked(date: string): void }) {
  const { board } = useView();
  const [view, setView] = useCalendarView();
  const today = isoDate(now);
  const shown = day(picked);
  const days =
    view === "week"
      ? Array.from({ length: 7 }, (_, i) => new Date(shown.getFullYear(), shown.getMonth(), shown.getDate() - ((shown.getDay() + 6) % 7) + i))
      : view === "month"
        ? monthGrid(shown.getFullYear(), shown.getMonth())
        : [];
  const isoDays = days.map(isoDate);
  // The year shows only what was done.
  const due = view === "year" ? new Map<string, DueOn[]>() : dueBetween(board, isoDays[0], isoDays[isoDays.length - 1]);
  const done = doneByDay(board);
  const turn = (by: number) => setPicked(turned(picked, view, by));
  const current = view === "year" ? picked.slice(0, 4) === today.slice(0, 4) : view === "week" ? isoDays.includes(today) : picked.slice(0, 7) === today.slice(0, 7);
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
  // Swiped sideways across the days, the view turns as with the arrows.
  const swipe = useRef<{ x: number; y: number } | null>(null);
  const swiped = (e: React.PointerEvent) => {
    const from = swipe.current;
    swipe.current = null;
    if (!from || e.pointerType === "mouse") return;
    const [dx, dy] = [e.clientX - from.x, e.clientY - from.y];
    if (Math.abs(dx) > 48 && Math.abs(dx) > 2 * Math.abs(dy)) turn(dx < 0 ? 1 : -1);
  };
  return (
    <div className="cal-page">
      <div className="cal-bar">
        <span aria-live="polite">{view === "year" ? `${shown.getFullYear()}年` : `${shown.getFullYear()}年${shown.getMonth() + 1}月`}</span>
        {!current && (
          <button type="button" className="text-btn" onClick={() => setPicked(today)}>
            今天
          </button>
        )}
        <Tip label={view === "year" ? "上一年" : view === "week" ? "上一周" : "上个月"}>
          <button type="button" className="icon-btn sm" onClick={() => turn(-1)}>
            <Icon of={ChevronLeft} size={18} />
          </button>
        </Tip>
        <Tip label={view === "year" ? "下一年" : view === "week" ? "下一周" : "下个月"}>
          <button type="button" className="icon-btn sm" onClick={() => turn(1)}>
            <Icon of={ChevronRight} size={18} />
          </button>
        </Tip>
      </div>
      <div ref={grid} onPointerDown={(e) => (swipe.current = { x: e.clientX, y: e.clientY })} onPointerUp={swiped} onPointerCancel={() => (swipe.current = null)}>
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
                setView("month");
              });
              focus(d);
            }}
          />
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
                const lunar = lunarDate(iso);
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
                    aria-label={`${d.getMonth() + 1}月${d.getDate()}日${lunar ? `，农历${lunarText(lunar)}` : ""}，${dots.length} 项截止${n ? `，完成 ${n} 项` : ""}`}
                    onClick={() => setPicked(iso)}
                    onKeyDown={(e) => move(e, d)}
                  >
                    <span className={n && !out ? `heat-${heat(n)}` : undefined}>
                      <span className="cal-num">{d.getDate()}</span>
                      <small>{lunarLabel(iso)}</small>
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
      {view !== "year" && <DayList date={picked} now={now} overdue={picked === today ? overdue(board, today) : []} due={due.get(picked) ?? []} done={done.get(picked) ?? []} />}
    </div>
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

/** What is due and done on `date`, and for today, what is `overdue`. */
function DayList({ date, now, overdue, due, done }: { date: string; now: Date; overdue: Task[]; due: DueOn[]; done: DoneOn[] }) {
  const { board } = useView();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const lunar = lunarDate(date);
  const title = `${formatToday(day(date))}${lunar ? ` · ${lunarText(lunar)}` : ""}`;
  return (
    <section className="cal-day">
      <h2 className="cal-date">{title}</h2>
      {!overdue.length && !due.length && !done.length && <p className="empty">这天没有事项</p>}
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
      {due.length > 0 && (
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

/** A time a repeating task is due later, or was `done`: not a task to tick, but one to open. */
function Occurrence({ task, meta, done, onOpen }: { task: Task; meta: string; done?: boolean; onOpen(): void }) {
  const { board } = useView();
  return (
    <div className="row task occurrence" onClick={onOpen}>
      <div className="row-inner">
        <span className={`check ${done ? "done" : "waiting"}`} aria-hidden>
          <Icon of={CheckIcon} size={15} width={2.6} />
        </span>
        <button type="button" className="task-main">
          <span className="task-head">
            <span className="task-title">{task.title}</span>
          </span>
          <span className="task-meta">
            {task.folder_id && <PathMeta board={board} folder={task.folder_id} />}
            <span>{meta}</span>
          </span>
        </button>
      </div>
    </div>
  );
}
