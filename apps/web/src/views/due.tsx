// The due date panel: a month to pick the day in, and rows for the time and repeat; and the start's.

import { ChevronLeft, ChevronRight, ChevronsUpDown } from "lucide-react";
import { Popover } from "radix-ui";
import { type ReactNode, useContext, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type { Due } from "../api/types/Due";
import type { DueAfter } from "../api/types/DueAfter";
import type { Repeat } from "../api/types/Repeat";
import type { RepeatFrom } from "../api/types/RepeatFrom";
import type { RepeatUnit } from "../api/types/RepeatUnit";
import { MAX_REMINDERS, day, formatDue, inOrder, isoDate, monthGrid, nextFive, parseTime, startLabel } from "../domain/dates";
import { dayName, lunarDate, lunarText } from "../domain/lunar";
import { WORKDAYS, daysBefore, plus, repeatLabel } from "../domain/repeat";
import { defaultReminders } from "../remind-settings";
import { useView } from "../session";
import { useBack } from "../ui/back";
import { Seg, Select, Stepper, Switch } from "../ui/controls";
import { Modal, RootContext, Tip } from "../ui/feedback";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { composing } from "../ui/keys";
import { DayNote, daySpoken, useHolidayMarks } from "./daynote";

const UNITS: [RepeatUnit, string][] = [
  ["day", "天"],
  ["week", "周"],
  ["month", "个月"],
];

/** Sets or clears the due date, its time and how it repeats; a task `endedAs` “已完成” or “已放弃”
 * doesn't repeat. Not before the task's `start`, at `startTime`. Or the due date some days after the
 * task's prerequisites are done, `dueAfter`, of which `due` is then the date given once they were
 * (docs/design/dates.md#前置完成后的截止). Its reminders by the due date keep, those so long before going with
 * the time; a task given one now gets this device's defaults, unless with the `others` it has, by the
 * start and at times of their own, that would be too many (docs/design/dates.md#提醒). */
export function DueEditor({
  due,
  dueAfter,
  start,
  startTime,
  endedAs,
  others,
  onSave,
  onClose,
}: {
  due: Due | null;
  dueAfter?: DueAfter;
  start: string | null;
  startTime?: string;
  endedAs: string | null;
  others: number;
  onSave(change: { due: Due | null } | { due_after: DueAfter }): void;
  onClose(): void;
}) {
  const now = useNow();
  const [date, setDate] = useState(() => {
    const today = isoDate(new Date());
    return due?.date ?? (start && start > today ? start : today);
  });
  const [after, setAfter] = useState(dueAfter !== undefined);
  const [afterDays, setAfterDays] = useState(dueAfter?.days ?? 7);
  const timing = dueAfter ?? due;
  const [time, setTime] = useState(timing?.time ?? "");
  const defaults = defaultReminders(useView().board);
  const was = due?.repeat;
  // One of the units, every one of them; a year, every 12 months; or every so many of a unit.
  const [kind, setKind] = useState<RepeatUnit | "year" | "custom" | "off">(
    !was ? "off" : was.every === 1 ? was.unit : was.every === 12 && was.unit === "month" && (!was.days || was.lunar) ? "year" : "custom",
  );
  const [every, setEvery] = useState(was?.every ?? 2);
  const [unit, setUnit] = useState<RepeatUnit>(was?.unit ?? "day");
  const [from, setFrom] = useState<RepeatFrom>(was?.from ?? "due");
  // A weekly repeat's days of the week, or a monthly one's of the month; null for a plain one, on the
  // due date's day whichever it is moved to. One by the lunar calendar has them always, by default the
  // due date's lunar day.
  const [days, setDays] = useState(was?.days ?? null);
  const [lunar, setLunar] = useState(was?.lunar ?? false);
  // Every statutory working day: weekly, Monday to Friday, by the days off and worked.
  const [legal, setLegal] = useState(was?.legal ?? false);
  // The last day it falls on, or none: on and on.
  const [until, setUntil] = useState(was?.until ?? null);
  const on = kind === "custom" ? unit : kind;
  const monthly = on === "month" || on === "year";
  const lunarDay = lunarDate(date);
  const shownDays = days ?? [lunar ? (lunarDay?.day ?? 1) : on === "week" ? ((day(date).getDay() + 6) % 7) + 1 : day(date).getDate()];
  const picked = lunar ? shownDays : (on === "week" || on === "month") && days ? days : undefined;
  const repeat: Repeat | null =
    kind === "off"
      ? null
      : {
          ...(kind === "custom" ? { every, unit } : kind === "year" ? { every: 12, unit: "month" } : { every: 1, unit: kind }),
          from,
          ...(picked && { days: picked }),
          ...(lunar && { lunar }),
          ...(legal && { legal }),
          ...(until && { until }),
        };
  // Days picked are a menu item of their own, named by them as 每个工作日 is: picking any other item is
  // then a change, all the menu reports, and starts over without them.
  const workdays = repeat?.every === 1 && repeat.unit === "week" && repeat.days?.join() === WORKDAYS.join();
  const named = repeat?.days && !workdays ? repeatLabel(repeat, date) : null;
  const menu = legal ? "legal" : workdays ? "workdays" : named ? "days" : kind;
  // Offered where the days off are marked, or kept as the task has it (docs/design/repeat.md#节日和放假).
  const marks = useHolidayMarks();
  // Said once saving is tried, until it is put right.
  const [tried, setTried] = useState(false);
  const early = Boolean(startTime && time && (after ? afterDays === 0 : date === start) && time < startTime);
  // Moved past the last day it repeats on.
  const ended = Boolean(repeat?.until && repeat.until < date);
  // By the lunar calendar, outside its table.
  const unlisted = lunar && !lunarDay;
  const save = () => {
    if (early || ended || unlisted) return setTried(true);
    const given = time ? defaults.timed : defaults.dated;
    const reminders = inOrder(timing ? (timing.reminders ?? []) : given.length + others > MAX_REMINDERS ? [] : given, time || undefined);
    const timed = { ...(time && { time }), ...(reminders.length && { reminders }) };
    onSave(after ? { due_after: { days: afterDays, ...timed } } : { due: { date, ...timed, ...(repeat && { repeat }) } });
    onClose();
  };
  return (
    <Modal
      title="截止时间"
      onClose={onClose}
      footer={
        <>
          {tried && (early || ended || unlisted) && (
            <p className="form-error" role="alert">
              {early ? "截止不能早于开始" : ended ? "重复到不能早于截止" : "按农历重复的截止日期应在 1900 年 1 月 31 日到 2101 年 1 月 28 日之间"}
            </p>
          )}
          {timing && (
            <button className="text-btn danger-text" onClick={() => (onSave({ due: null }), onClose())}>
              清除截止时间
            </button>
          )}
          <button className="filled-btn" onClick={save}>
            保存
          </button>
        </>
      }
    >
      <div className="due-mode">
        <Seg<"date" | "after">
          label="截止方式"
          items={[
            ["date", "按日期"],
            ["after", "前置完成后"],
          ]}
          value={after ? "after" : "date"}
          disabled={repeat ? ["after"] : undefined}
          onChange={(m) => setAfter(m === "after")}
        />
      </div>
      {repeat && <p className="group-note">重复的事项按日期算截止</p>}
      {after ? (
        <>
          <div className="group settings">
            <Controls name="几天内">
              <Stepper label="几天内" min={0} max={365} value={afterDays} onChange={setAfterDays} />
            </Controls>
          </div>
          <p className="group-note">
            前置都完成的那天起算，0 是当天
            {dueAfter && due && (
              <>
                <br />
                已定下：{formatDue({ date: due.date }, now)}
              </>
            )}
          </p>
        </>
      ) : (
        <Calendar value={date} min={start ?? undefined} onChange={setDate} />
      )}
      <div className="group settings">
        <TimeRow value={time} extra={timing?.time} min={(after ? afterDays === 0 : date === start) ? startTime : undefined} onChange={setTime} optional />
        {!after && <Select<typeof menu>
          label="重复"
          row
          items={[
            ["off", endedAs ? `${endedAs}的事项不能重复` : "不重复"],
            ["day", "每天"],
            ["workdays", "每个工作日"],
            ...(marks || menu === "legal" ? [["legal", "每个法定工作日"] satisfies [typeof menu, string]] : []),
            ["week", "每周"],
            ...(named ? [["days", named] satisfies [typeof menu, string]] : []),
            ["month", "每月"],
            ["year", "每年"],
            ["custom", "自定义"],
          ]}
          value={menu}
          disabled={endedAs !== null}
          onChange={(k) => {
            // For the type: "days" is listed only while it is the value, which isn't picked anew.
            if (k === "days") return;
            setKind(k === "workdays" || k === "legal" ? "week" : k);
            setDays(k === "workdays" || k === "legal" ? WORKDAYS : null);
            setLunar(false);
            setLegal(k === "legal");
          }}
        />}
        {kind === "custom" && (
          <Controls name="间隔">
            <Stepper label="间隔数" min={1} value={every} max={365} onChange={setEvery} />
            <Seg<RepeatUnit>
              label="间隔单位"
              small
              items={UNITS}
              value={unit}
              onChange={(u) => {
                setUnit(u);
                setDays(null);
                setLunar(false);
              }}
            />
          </Controls>
        )}
        {on === "week" && !legal && (
          <Controls name="周几">
            <Days label="周几" texts={[...WEEKDAYS]} name={(t) => `周${t}`} value={shownDays} onChange={setDays} />
          </Controls>
        )}
        {monthly && (
          <Switch
            label="按农历"
            sub={lunarDay ? `截止日期是农历${lunarText(lunarDay)}` : "截止日期不在农历的范围里"}
            on={lunar}
            onChange={(to) => {
              setLunar(to);
              setDays(null);
            }}
          />
        )}
        {lunar ? (
          <Controls name="几号">
            <Days label="几号" lunar texts={LUNAR_DAYS} name={(t) => t} value={shownDays} onChange={setDays} />
          </Controls>
        ) : (
          on === "month" && (
            <Controls name="几号">
              <Days label="几号" texts={MONTHDAYS} name={(t) => `${t} 号`} value={shownDays} onChange={setDays} />
            </Controls>
          )
        )}
        {repeat && (
          <Controls name="下一次从">
            <Seg<RepeatFrom>
              label="下一次从"
              small
              items={[
                ["due", "截止日期算"],
                ["done", "完成那天算"],
              ]}
              value={from}
              onChange={setFrom}
            />
          </Controls>
        )}
        {repeat && <UntilRow value={until} min={date} onChange={setUntil} />}
      </div>
    </Modal>
  );
}

export const WEEKDAYS = "一二三四五六日";
const monthOf = (d: Date) => ({ year: d.getFullYear(), month: d.getMonth() });
/** How many days each arrow key moves a calendar's day by. */
export const ARROWS = new Map([
  ["ArrowLeft", -1],
  ["ArrowRight", 1],
  ["ArrowUp", -7],
  ["ArrowDown", 7],
]);

/** The longest a start is counted back from the due date. */
const MAX_AHEAD = 365;

/** Sets when a task starts, a day and maybe a time, not after it is `due`; or clears it. The day is
 * picked, or counted back from the due date, as a repeating one opens: its start is kept as far before
 * it each time (docs/design/dates.md#开始时间). Due some days after its prerequisites (`dueAfter`), it counts
 * from a later start, so that only a time on the day itself, 0 days after, may be too late. */
export function StartEditor({
  start,
  time: was,
  due,
  dueAfter,
  onSave,
  onClose,
}: {
  start: string | null;
  time?: string;
  due: Due | null;
  dueAfter?: DueAfter;
  onSave(start: { date: string; time?: string } | null): void;
  onClose(): void;
}) {
  const now = useNow();
  const bound = dueAfter ? null : due;
  const [date, setDate] = useState(() => {
    const today = isoDate(new Date());
    return start ?? (bound && bound.date < today ? bound.date : today);
  });
  // How many days before the due date, while counting back from it: as a repeating task's opens, unless
  // its start is further back than that counts.
  const [ahead, setAhead] = useState<number | null>(() => {
    const back = bound?.repeat ? (start ? daysBefore(start, bound.date) : 0) : -1;
    return back >= 0 && back <= MAX_AHEAD ? back : null;
  });
  // No longer due on a date, changed in another window say, it is the day picked again.
  const counting = bound !== null && ahead !== null;
  const startDay = counting ? plus(bound.date, -ahead) : date;
  const [time, setTime] = useState(was ?? "");
  // Said once saving is tried, until it is put right.
  const [tried, setTried] = useState(false);
  const latest = dueAfter?.days === 0 ? dueAfter.time : startDay === bound?.date ? bound.time : undefined;
  const late = Boolean(latest && time && time > latest) || (bound !== null && startDay > bound.date);
  const save = () => {
    if (late) return setTried(true);
    onSave({ date: startDay, ...(time && { time }) });
    onClose();
  };
  return (
    <Modal
      title="开始时间"
      onClose={onClose}
      footer={
        <>
          {tried && late && (
            <p className="form-error" role="alert">
              开始不能晚于截止
            </p>
          )}
          {start && (
            <button className="text-btn danger-text" onClick={() => (onSave(null), onClose())}>
              清除开始时间
            </button>
          )}
          <button className="filled-btn" onClick={save}>
            保存
          </button>
        </>
      }
    >
      <p className="group-note">开始之前不在“接下来”和小组件里显示</p>
      {bound && (
        <div className="due-mode">
          <Seg<"date" | "before">
            label="开始方式"
            items={[
              ["date", "按日期"],
              ["before", "截止前"],
            ]}
            value={counting ? "before" : "date"}
            // The same day, the other way.
            onChange={(m) => (m === "before" ? setAhead(Math.min(Math.max(daysBefore(date, bound.date), 0), MAX_AHEAD)) : (setDate(startDay), setAhead(null)))}
          />
        </div>
      )}
      {!counting ? (
        // A day picked is the start, whatever becomes of the due date meanwhile.
        <Calendar value={date} max={bound?.date} onChange={(d) => (setDate(d), setAhead(null))} />
      ) : (
        <>
          <div className="group settings">
            <Controls name="提前几天">
              <Stepper label="提前几天" min={0} max={MAX_AHEAD} value={ahead} onChange={setAhead} />
            </Controls>
          </div>
          <p className="group-note">{ahead === 0 ? "截止当天开始" : startLabel(startDay, undefined, now)}</p>
        </>
      )}
      <div className="group settings">
        <TimeRow value={time} extra={was} max={latest} onChange={setTime} optional />
      </div>
    </Modal>
  );
}

/** A month of days, Monday first, to pick `value` from, if any, between `min` and `max`; the days
 * around it in the six weeks shown are there to pick too. With no day picked, the month of `min`, or
 * this month, is shown. `onPicked` is told of a day clicked, not of one moved to with the arrow keys. */
export function Calendar({
  value,
  min,
  max,
  onChange,
  onPicked,
}: {
  value: string;
  min?: string;
  max?: string;
  onChange(date: string): void;
  onPicked?(date: string): void;
}) {
  const today = isoDate(useNow());
  const marks = useHolidayMarks();
  const [shown, setShown] = useState(() => monthOf(day(value || (min && min > today ? min : today))));
  const out = (iso: string) => (min !== undefined && iso < min) || (max !== undefined && iso > max);
  const days = useRef<HTMLDivElement>(null);
  const weeks = monthGrid(shown.year, shown.month);
  // Tab comes to the day picked, or with none there to the first of the month that can be.
  const tabStop =
    weeks.some((d) => isoDate(d) === value) && !out(value) ? value : isoDate(weeks.find((d) => d.getMonth() === shown.month && !out(isoDate(d))) ?? new Date(shown.year, shown.month, 1));
  const pick = (d: Date) => {
    onChange(isoDate(d));
    setShown(monthOf(d));
  };
  const turn = (by: number) => setShown(monthOf(new Date(shown.year, shown.month + by)));
  const move = (e: React.KeyboardEvent, from: Date) => {
    const by = ARROWS.get(e.key);
    if (by === undefined) return;
    e.preventDefault();
    const to = new Date(from.getFullYear(), from.getMonth(), from.getDate() + by);
    if (out(isoDate(to))) return;
    // Shown at once, in its month, to take the focus.
    flushSync(() => pick(to));
    days.current!.querySelector<HTMLElement>(`[data-date="${isoDate(to)}"]`)!.focus();
  };
  return (
    <div className="group cal">
      <div className="cal-head">
        <span aria-live="polite">
          {shown.year}年{shown.month + 1}月
        </span>
        <Tip label="上个月">
          <button className="icon-btn sm" onClick={() => turn(-1)}>
            <Icon of={ChevronLeft} size={18} />
          </button>
        </Tip>
        <Tip label="下个月">
          <button className="icon-btn sm" onClick={() => turn(1)}>
            <Icon of={ChevronRight} size={18} />
          </button>
        </Tip>
      </div>
      <div className="cal-week" aria-hidden>
        {[...WEEKDAYS].map((w) => (
          <span key={w}>{w}</span>
        ))}
      </div>
      <div className="cal-days" ref={days}>
        {weeks.map((d) => {
          const iso = isoDate(d);
          return (
            <button
              key={iso}
              data-date={iso}
              className={d.getMonth() === shown.month ? undefined : "out"}
              tabIndex={iso === tabStop ? 0 : -1}
              aria-pressed={iso === value}
              aria-current={iso === today ? "date" : undefined}
              disabled={out(iso)}
              aria-label={`${d.getMonth() + 1}月${d.getDate()}日${daySpoken(iso, marks)}`}
              onClick={() => (pick(d), onPicked?.(iso))}
              onKeyDown={(e) => move(e, d)}
            >
              <span>
                {d.getDate()}
                <DayNote date={iso} marks={marks} />
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

const pad = (n: number) => String(n).padStart(2, "0");
const HOURS = Array.from({ length: 24 }, (_, h) => h);
const FIVES = Array.from({ length: 12 }, (_, i) => i * 5);

/** The time of day, `HH:MM`: typed, or the hour, then the minute, every 5, from two columns. The
 * minutes of `extra`, the time it was, and of the time now are there to pick too. `optional`, it can
 * be none, "": opened then, it starts at the next five minutes, not before `min` nor after `max`, to take
 * as it is or change. */
export function TimeRow({
  label = "时间",
  value,
  extra,
  optional = false,
  min,
  max,
  onChange,
}: {
  label?: string;
  value: string;
  extra?: string;
  optional?: boolean;
  min?: string;
  max?: string;
  onChange(time: string): void;
}) {
  const root = useContext(RootContext);
  const [open, setOpen] = useState(false);
  useBack("dialog", open ? () => setOpen(false) : null);
  const hours = useRef<HTMLDivElement>(null);
  const mins = useRef<HTMLDivElement>(null);
  const hour = value ? Number(value.slice(0, 2)) : null;
  const minute = value ? Number(value.slice(3)) : null;
  const minutes = [...new Set([...FIVES, ...[extra, value].filter((t) => t).map((t) => Number(t!.slice(3)))])].sort((a, b) => a - b);
  // Picked from the columns, what was typed is no longer the time: the field starts afresh.
  const [picks, setPicks] = useState(0);
  const pick = (time: string) => (onChange(time), setPicks((n) => n + 1));
  // Each column at what is picked, the hours at `h` if none is.
  const scroll = (h: number) => {
    const centre = (col: HTMLElement, el: HTMLElement) => (col.scrollTop = el.offsetTop - (col.clientHeight - el.offsetHeight) / 2);
    const at = hours.current!.querySelectorAll("button")[h];
    centre(hours.current!, at);
    const m = mins.current!.querySelector<HTMLElement>(".on");
    if (m) centre(mins.current!, m);
    return at;
  };
  const show = (e: Event) => {
    e.preventDefault();
    scroll(hour!).focus({ preventScroll: true });
  };
  // Typed, the columns show it at once: its minute may be a new one.
  const typed = (time: string) => {
    flushSync(() => onChange(time));
    scroll(Number(time.slice(0, 2)));
  };
  return (
    <Popover.Root
      open={open}
      onOpenChange={(o) => {
        // In the same render as opening, so that `show` finds the hour picked to focus.
        if (o && !value) {
          const next = nextFive(new Date());
          onChange(min && next < min ? min : max && next > max ? max : next);
        }
        setOpen(o);
      }}
      modal
    >
      <Popover.Trigger className="row pick">
        <div className="row-inner">
          {label}
          <span className="row-value">{value || "不设时间"}</span>
          <Icon of={ChevronsUpDown} size={16} />
        </div>
      </Popover.Trigger>
      <Popover.Portal container={root}>
        <Popover.Content className="menu time-menu" align="end" sideOffset={4} onOpenAutoFocus={show}>
          <TimeInput key={picks} onChange={typed} onDone={() => setOpen(false)} />
          <div className="time-cols">
            <div ref={hours} role="group" aria-label="时">
              {HOURS.map((h) => (
                <button key={h} className={h === hour ? "on" : undefined} aria-pressed={h === hour} onClick={() => pick(`${pad(h)}:${pad(minute ?? 0)}`)}>
                  {pad(h)}
                </button>
              ))}
            </div>
            <div ref={mins} role="group" aria-label="分">
              {minutes.map((m) => (
                <button
                  key={m}
                  className={m === minute ? "on" : undefined}
                  aria-pressed={m === minute}
                  disabled={hour === null}
                  onClick={() => (pick(`${pad(hour!)}:${pad(m)}`), setOpen(false))}
                >
                  {pad(m)}
                </button>
              ))}
            </div>
          </div>
          {optional && (
            <button className="menu-item" onClick={() => (onChange(""), setOpen(false))}>
              不设时间
            </button>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** The last day a repeat falls on, not before `min`, the due date; or none, on and on. */
function UntilRow({ value, min, onChange }: { value: string | null; min: string; onChange(until: string | null): void }) {
  const root = useContext(RootContext);
  const now = useNow();
  const [open, setOpen] = useState(false);
  useBack("dialog", open ? () => setOpen(false) : null);
  const content = useRef<HTMLDivElement>(null);
  // Opened, the day picked, or the first of the month shown that can be, takes the focus for the
  // arrow keys.
  const show = (e: Event) => {
    e.preventDefault();
    content.current!.querySelector<HTMLElement>('.cal-days [tabindex="0"]')!.focus({ preventScroll: true });
  };
  return (
    <Popover.Root open={open} onOpenChange={setOpen} modal>
      <Popover.Trigger className="row pick">
        <div className="row-inner">
          重复到
          <span className="row-value">{value ? formatDue({ date: value }, now) : "一直重复"}</span>
          <Icon of={ChevronsUpDown} size={16} />
        </div>
      </Popover.Trigger>
      <Popover.Portal container={root}>
        <Popover.Content ref={content} className="menu until-menu" align="end" sideOffset={4} collisionPadding={8} onOpenAutoFocus={show}>
          <Calendar value={value ?? ""} min={min} onChange={onChange} onPicked={() => setOpen(false)} />
          <button className="menu-item" onClick={() => (onChange(null), setOpen(false))}>
            一直重复
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** A time typed, taken as it is typed, since the number pads of some phones have no Enter; Enter is
 * done with it. */
function TimeInput({ onChange, onDone }: { onChange(time: string): void; onDone(): void }) {
  const [text, setText] = useState("");
  const [bad, setBad] = useState(false);
  const type = (typed: string) => {
    setText(typed);
    setBad(false);
    const time = parseTime(typed);
    if (time) onChange(time);
  };
  const enter = (e: React.KeyboardEvent) => {
    if (e.key !== "Enter" || composing(e)) return;
    e.preventDefault();
    if (parseTime(text)) onDone();
    else setBad(true);
  };
  return (
    <input
      className="time-input"
      value={text}
      onChange={(e) => type(e.target.value)}
      onKeyDown={enter}
      placeholder="输入，如 9:07"
      aria-label="输入时间"
      aria-invalid={bad}
      // Digits on a phone, and a dot to part the hour from the minutes: few number pads have a colon.
      inputMode="decimal"
      autoComplete="off"
    />
  );
}

const MONTHDAYS = Array.from({ length: 31 }, (_, i) => String(i + 1));
const LUNAR_DAYS = Array.from({ length: 30 }, (_, i) => dayName(i + 1));

/** A row named on the left with controls for its value on the right, or under the name, to the right,
 * where the two don't fit side by side: a narrow screen, or large text. */
export function Controls({ name, children }: { name: string; children: ReactNode }) {
  return (
    <div className="row">
      <div className="row-inner controls">
        {name}
        <span className="row-value">{children}</span>
      </div>
    </div>
  );
}

/** The days a repeat falls on, seven a line: Monday 1 to Sunday 7, the 1st to the 31st, or 初一 to 三十
 * of a `lunar` month. `texts` are on the buttons, `name` says them in full. The last one picked stays. */
function Days(p: { label: string; lunar?: boolean; texts: string[]; name(text: string): string; value: number[]; onChange(days: number[]): void }) {
  return (
    <span className={p.lunar ? "days lunar" : "days"} role="group" aria-label={p.label}>
      {p.texts.map((text, i) => {
        const on = p.value.includes(i + 1);
        return (
          <button
            key={text}
            aria-pressed={on}
            aria-label={p.name(text)}
            disabled={on && p.value.length === 1}
            onClick={() => p.onChange(on ? p.value.filter((d) => d !== i + 1) : [...p.value, i + 1].sort((a, b) => a - b))}
          >
            {text}
          </button>
        );
      })}
    </span>
  );
}
