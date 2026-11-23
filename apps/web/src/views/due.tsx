// The due date panel: a month to pick the day in, and rows for the time, reminder and repeat.

import { ChevronLeft, ChevronRight, ChevronsUpDown } from "lucide-react";
import { Popover } from "radix-ui";
import { type ReactNode, useContext, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type { Due } from "../api/types/Due";
import type { Remind } from "../api/types/Remind";
import type { Repeat } from "../api/types/Repeat";
import type { RepeatFrom } from "../api/types/RepeatFrom";
import type { RepeatUnit } from "../api/types/RepeatUnit";
import {
  DAY_REMINDERS,
  MAX_REMIND,
  MAX_REMINDERS,
  MAX_REMIND_DAYS,
  REMINDERS,
  REMIND_UNITS,
  type RemindUnit,
  day,
  formatDue,
  inOrder,
  isoDate,
  monthGrid,
  parseTime,
  remindLabel,
  remindersLabel,
} from "../domain/dates";
import { dayName, lunarDate, lunarLabel, lunarText } from "../domain/lunar";
import { WORKDAYS, repeatLabel } from "../domain/repeat";
import { useDefaultReminders } from "../remind-settings";
import { useBack } from "../ui/back";
import { MultiSelect, Seg, Select, Stepper, Switch } from "../ui/controls";
import { Modal, RootContext, Tip } from "../ui/feedback";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { composing } from "../ui/keys";

const UNITS: [RepeatUnit, string][] = [
  ["day", "天"],
  ["week", "周"],
  ["month", "个月"],
];
const AHEAD = (["minute", "hour", "day"] as const).map((u): [RemindUnit, string] => [u, REMIND_UNITS[u].name]);

/** Sets or clears the due date, its time, the reminder and how it repeats; a task `done` doesn't
 * repeat. Not before the task's `start`, at `startTime`. */
export function DueEditor({
  due,
  start,
  startTime,
  done,
  onSave,
  onClose,
}: {
  due: Due | null;
  start: string | null;
  startTime?: string;
  done: boolean;
  onSave(d: Due | null): void;
  onClose(): void;
}) {
  const [date, setDate] = useState(() => {
    const today = isoDate(new Date());
    return due?.date ?? (start && start > today ? start : today);
  });
  const [time, setTime] = useState(due?.time ?? "");
  // As picked; for a task given a due date now, until they are, this device's defaults for a due
  // date with a time or without (docs/design.md#提醒). Those before the time go with it.
  const defaults = useDefaultReminders();
  const [chosen, setChosen] = useState<Remind[] | null>(due ? (due.reminders ?? []) : null);
  const reminders = inOrder(chosen ?? (time ? defaults.timed : defaults.dated), time || undefined);
  // One not on the menu being added: so far ahead of the time, or on a day before at a time.
  const [adding, setAdding] = useState(false);
  const [byDay, setByDay] = useState(false);
  const [ahead, setAhead] = useState<[number, RemindUnit]>([10, "minute"]);
  const [onDay, setOnDay] = useState<[number, string]>([1, "09:00"]);
  const add = () => {
    setChosen([...reminders, time && !byDay ? { before: ahead[0] * REMIND_UNITS[ahead[1]].minutes } : { days: onDay[0], time: onDay[1] }]);
    setAdding(false);
  };
  // From the due time back, as the menu lists them.
  const offered = inOrder([...(time ? REMINDERS.map((before) => ({ before })) : DAY_REMINDERS), ...reminders], time || undefined).reverse();
  const key = (r: Remind) => ("before" in r ? `before ${r.before}` : `${r.days} days ${r.time}`);
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
          ...(until && { until }),
        };
  // Days picked are a menu item of their own, named by them as 每个工作日 is: picking any other item is
  // then a change, all the menu reports, and starts over without them.
  const workdays = repeat?.every === 1 && repeat.unit === "week" && repeat.days?.join() === WORKDAYS.join();
  const named = repeat?.days && !workdays ? repeatLabel(repeat, date) : null;
  const menu = workdays ? "workdays" : named ? "days" : kind;
  // Said once saving is tried, until it is put right.
  const [tried, setTried] = useState(false);
  const early = Boolean(startTime && time && date === start && time < startTime);
  // Moved past the last day it repeats on.
  const ended = Boolean(repeat?.until && repeat.until < date);
  // By the lunar calendar, outside its table.
  const unlisted = lunar && !lunarDay;
  const tooMany = reminders.length > MAX_REMINDERS;
  const save = () => {
    if (early || ended || unlisted || tooMany) return setTried(true);
    onSave({ date, ...(time && { time }), ...(reminders.length && { reminders }), ...(repeat && { repeat }) });
    onClose();
  };
  return (
    <Modal
      title="截止时间"
      onClose={onClose}
      footer={
        <>
          {tried && (early || ended || unlisted || tooMany) && (
            <p className="form-error" role="alert">
              {early
                ? "截止不能早于开始"
                : ended
                  ? "重复到不能早于截止"
                  : unlisted
                    ? "按农历重复的截止日期应在 1900 年 1 月 31 日到 2101 年 1 月 28 日之间"
                    : `提醒最多 ${MAX_REMINDERS} 个`}
            </p>
          )}
          {due && (
            <button className="text-btn danger-text" onClick={() => (onSave(null), onClose())}>
              清除截止时间
            </button>
          )}
          <button className="filled-btn" onClick={save}>
            保存
          </button>
        </>
      }
    >
      <Calendar value={date} min={start ?? undefined} onChange={setDate} />
      <div className="group settings">
        <TimeRow value={time} extra={due?.time} onChange={setTime} optional />
        <MultiSelect
          label="提醒"
          text={remindersLabel(reminders)}
          items={offered.map((r) => [key(r), remindLabel(r)])}
          value={reminders.map(key)}
          onChange={(keys) => setChosen(offered.filter((r) => keys.includes(key(r))))}
          none="不提醒"
          more={["自定义…", () => setAdding(true)]}
        />
        {adding && time && (
          <Controls name="提醒方式">
            <Seg<"ahead" | "day">
              label="提醒方式"
              small
              items={[
                ["ahead", "提前多久"],
                ["day", "按日子"],
              ]}
              value={byDay ? "day" : "ahead"}
              onChange={(k) => setByDay(k === "day")}
            />
          </Controls>
        )}
        {adding &&
          (time && !byDay ? (
            <Controls name="提前">
              <Stepper label="提前多少" min={1} value={ahead[0]} max={MAX_REMIND / REMIND_UNITS[ahead[1]].minutes} onChange={(n) => setAhead([n, ahead[1]])} />
              <Seg<RemindUnit>
                label="提前的单位"
                small
                items={AHEAD}
                value={ahead[1]}
                onChange={(u) => setAhead([Math.min(ahead[0], MAX_REMIND / REMIND_UNITS[u].minutes), u])}
              />
            </Controls>
          ) : (
            <>
              <Controls name="提前几天">
                <Stepper label="提前几天" min={0} value={onDay[0]} max={MAX_REMIND_DAYS} onChange={(n) => setOnDay([n, onDay[1]])} />
              </Controls>
              <TimeRow label="几点" value={onDay[1]} onChange={(t) => setOnDay([onDay[0], t])} />
            </>
          ))}
        {adding && (
          <Controls name="">
            <button className="text-btn" onClick={add}>
              添加
            </button>
          </Controls>
        )}
        <Select<typeof menu>
          label="重复"
          row
          items={[
            ["off", done ? "已完成的事项不能重复" : "不重复"],
            ["day", "每天"],
            ["workdays", "每个工作日"],
            ["week", "每周"],
            ...(named ? [["days", named] satisfies [typeof menu, string]] : []),
            ["month", "每月"],
            ["year", "每年"],
            ["custom", "自定义"],
          ]}
          value={menu}
          disabled={done}
          onChange={(k) => {
            // For the type: "days" is listed only while it is the value, which isn't picked anew.
            if (k === "days") return;
            setKind(k === "workdays" ? "week" : k);
            setDays(k === "workdays" ? WORKDAYS : null);
            setLunar(false);
          }}
        />
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
        {on === "week" && (
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

/** Sets when a task starts, a day and maybe a time, not after it is `due`; or clears it. */
export function StartEditor({
  start,
  time: was,
  due,
  onSave,
  onClose,
}: {
  start: string | null;
  time?: string;
  due: Due | null;
  onSave(start: { date: string; time?: string } | null): void;
  onClose(): void;
}) {
  const [date, setDate] = useState(() => {
    const today = isoDate(new Date());
    return start ?? (due && due.date < today ? due.date : today);
  });
  const [time, setTime] = useState(was ?? "");
  // Said once saving is tried, until it is put right.
  const [tried, setTried] = useState(false);
  const late = Boolean(due?.time && time && date === due.date && time > due.time);
  const save = () => {
    if (late) return setTried(true);
    onSave({ date, ...(time && { time }) });
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
      <Calendar value={date} max={due?.date} onChange={setDate} />
      <div className="group settings">
        <TimeRow value={time} extra={was} onChange={setTime} optional />
      </div>
    </Modal>
  );
}

/** A month of days, Monday first, to pick `value` from, if any, between `min` and `max`; the days
 * around it in the six weeks shown are there to pick too. With no day picked, the month of `min`, or
 * this month, is shown. `onPicked` is told of a day clicked, not of one moved to with the arrow keys. */
function Calendar({
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
  onPicked?(): void;
}) {
  const today = isoDate(useNow());
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
          const l = lunarDate(iso);
          return (
            <button
              key={iso}
              data-date={iso}
              className={d.getMonth() === shown.month ? undefined : "out"}
              tabIndex={iso === tabStop ? 0 : -1}
              aria-pressed={iso === value}
              aria-current={iso === today ? "date" : undefined}
              disabled={out(iso)}
              aria-label={`${d.getMonth() + 1}月${d.getDate()}日${l ? `，农历${lunarText(l)}` : ""}`}
              onClick={() => (pick(d), onPicked?.())}
              onKeyDown={(e) => move(e, d)}
            >
              <span>
                {d.getDate()}
                <small aria-hidden>{lunarLabel(iso)}</small>
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
 * be none, "". */
export function TimeRow({
  label = "时间",
  value,
  extra,
  optional = false,
  onChange,
}: {
  label?: string;
  value: string;
  extra?: string;
  optional?: boolean;
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
  // Opened, at this hour if none is picked; that hour takes the focus.
  const show = (e: Event) => {
    e.preventDefault();
    scroll(hour ?? new Date().getHours()).focus({ preventScroll: true });
  };
  // Typed, the columns show it at once: its minute may be a new one.
  const typed = (time: string) => {
    flushSync(() => onChange(time));
    scroll(Number(time.slice(0, 2)));
  };
  return (
    <Popover.Root open={open} onOpenChange={setOpen} modal>
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
function Controls({ name, children }: { name: string; children: ReactNode }) {
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
