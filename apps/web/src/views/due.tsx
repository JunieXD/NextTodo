// The due date panel: a month to pick the day in, and rows for the time, reminder and repeat.

import { ChevronLeft, ChevronRight, ChevronsUpDown, Minus, Plus } from "lucide-react";
import { Popover } from "radix-ui";
import { useContext, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type { Due } from "../api/types/Due";
import type { Repeat } from "../api/types/Repeat";
import type { RepeatFrom } from "../api/types/RepeatFrom";
import type { RepeatUnit } from "../api/types/RepeatUnit";
import { REMINDERS, day, isoDate, remindLabel } from "../domain/dates";
import { WORKDAYS, repeatLabel } from "../domain/repeat";
import { useBack } from "../ui/back";
import { Seg, Select } from "../ui/controls";
import { Modal, RootContext, Tip } from "../ui/feedback";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";

const UNITS: [RepeatUnit, string][] = [
  ["day", "天"],
  ["week", "周"],
  ["month", "个月"],
];

/** Sets or clears the due date, its time, the reminder and how it repeats; a task `done` doesn't
 * repeat. */
export function DueEditor({ due, done, onSave, onClose }: { due: Due | null; done: boolean; onSave(d: Due | null): void; onClose(): void }) {
  const [date, setDate] = useState(due?.date ?? isoDate(new Date()));
  const [time, setTime] = useState(due?.time ?? "");
  const [remind, setRemind] = useState(due?.remind_before?.toString() ?? "off");
  const was = due?.repeat;
  // One of the units, every one of them; a year, every 12 months; or every so many of a unit.
  const [kind, setKind] = useState<RepeatUnit | "year" | "custom" | "off">(
    !was ? "off" : was.every === 1 ? was.unit : was.every === 12 && was.unit === "month" && !was.days ? "year" : "custom",
  );
  const [every, setEvery] = useState(was?.every ?? 2);
  const [unit, setUnit] = useState<RepeatUnit>(was?.unit ?? "day");
  const [from, setFrom] = useState<RepeatFrom>(was?.from ?? "due");
  // A weekly repeat's days of the week, or a monthly one's of the month; null for a plain one, on the
  // due date's day whichever it is moved to.
  const [days, setDays] = useState(was?.days ?? null);
  const on = kind === "custom" ? unit : kind;
  const shownDays = days ?? [on === "week" ? ((day(date).getDay() + 6) % 7) + 1 : day(date).getDate()];
  const picked = (on === "week" || on === "month") && days ? days : undefined;
  const repeat: Repeat | null =
    kind === "off"
      ? null
      : {
          ...(kind === "custom" ? { every, unit } : kind === "year" ? { every: 12, unit: "month" } : { every: 1, unit: kind }),
          from,
          ...(picked && { days: picked }),
        };
  // Days picked are a menu item of their own, named by them as 每个工作日 is: picking any other item is
  // then a change, all the menu reports, and starts over without them.
  const workdays = repeat?.every === 1 && repeat.days?.join() === WORKDAYS.join();
  const named = repeat?.days && !workdays ? repeatLabel(repeat) : null;
  const menu = workdays ? "workdays" : named ? "days" : kind;
  // A reminder set elsewhere (say, by an agent) may not be one of the usual choices.
  const choices = [...new Set([...REMINDERS, ...(due?.remind_before === undefined ? [] : [due.remind_before])])].sort((a, b) => a - b);
  const save = () => {
    onSave({ date, ...(time && { time }), ...(time && remind !== "off" && { remind_before: Number(remind) }), ...(repeat && { repeat }) });
    onClose();
  };
  return (
    <Modal title="截止时间" onClose={onClose}>
      <Calendar value={date} onChange={setDate} />
      <div className="group settings">
        <TimeRow value={time} extra={due?.time} onChange={setTime} />
        <Select
          label="提醒"
          row
          items={[["off", time ? "不提醒" : "设置时间后才能提醒"], ...choices.map((m): [string, string] => [String(m), remindLabel(m)])]}
          value={time ? remind : "off"}
          disabled={!time}
          onChange={setRemind}
        />
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
          }}
        />
        {kind === "custom" && (
          <div className="row">
            <div className="row-inner">
              间隔
              <span className="row-value">
                <Stepper value={every} onChange={setEvery} />
                <Seg<RepeatUnit>
                  label="间隔单位"
                  small
                  items={UNITS}
                  value={unit}
                  onChange={(u) => {
                    setUnit(u);
                    setDays(null);
                  }}
                />
              </span>
            </div>
          </div>
        )}
        {on === "week" && (
          <div className="row">
            <div className="row-inner">
              周几
              <span className="row-value">
                <Days label="周几" texts={[...WEEKDAYS]} name={(t) => `周${t}`} value={shownDays} onChange={setDays} />
              </span>
            </div>
          </div>
        )}
        {on === "month" && (
          <div className="row">
            <div className="row-inner">
              几号
              <span className="row-value">
                <Days label="几号" texts={MONTHDAYS} name={(t) => `${t} 号`} value={shownDays} onChange={setDays} />
              </span>
            </div>
          </div>
        )}
        {repeat && (
          <div className="row">
            <div className="row-inner">
              下一次从
              <span className="row-value">
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
              </span>
            </div>
          </div>
        )}
      </div>
      <div className="actions">
        {due && (
          <button className="text-btn danger-text" onClick={() => (onSave(null), onClose())}>
            清除截止时间
          </button>
        )}
        <button className="filled-btn" onClick={save}>
          保存
        </button>
      </div>
    </Modal>
  );
}

const WEEKDAYS = "一二三四五六日";
const monthOf = (d: Date) => ({ year: d.getFullYear(), month: d.getMonth() });
const ARROWS = new Map([
  ["ArrowLeft", -1],
  ["ArrowRight", 1],
  ["ArrowUp", -7],
  ["ArrowDown", 7],
]);

/** A month of days, Monday first, to pick `value` from; the days around it in the six weeks shown
 * are there to pick too. */
function Calendar({ value, onChange }: { value: string; onChange(date: string): void }) {
  const today = isoDate(useNow());
  const [shown, setShown] = useState(() => monthOf(day(value)));
  const days = useRef<HTMLDivElement>(null);
  const first = new Date(shown.year, shown.month, 1);
  const start = 1 - ((first.getDay() + 6) % 7);
  const weeks = Array.from({ length: 42 }, (_, i) => new Date(shown.year, shown.month, start + i));
  // Tab comes to the day picked, or to the 1st with that day in another month.
  const tabStop = weeks.some((d) => isoDate(d) === value) ? value : isoDate(first);
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
              aria-label={`${d.getMonth() + 1}月${d.getDate()}日`}
              onClick={() => pick(d)}
              onKeyDown={(e) => move(e, d)}
            >
              <span>{d.getDate()}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

const pad = (n: number) => String(n).padStart(2, "0");
const HOURS = Array.from({ length: 24 }, (_, h) => h);

/** The time of day, `HH:MM`, or none: the hour, then the minute, every 5, from two columns. The minute
 * of `extra`, the time it was, is there to pick too. */
function TimeRow({ value, extra, onChange }: { value: string; extra?: string; onChange(time: string): void }) {
  const root = useContext(RootContext);
  const [open, setOpen] = useState(false);
  useBack("dialog", open ? () => setOpen(false) : null);
  const hours = useRef<HTMLDivElement>(null);
  const mins = useRef<HTMLDivElement>(null);
  const hour = value ? Number(value.slice(0, 2)) : null;
  const minute = value ? Number(value.slice(3)) : null;
  const minutes = [...new Set([...Array.from({ length: 12 }, (_, i) => i * 5), ...(extra ? [Number(extra.slice(3))] : [])])].sort((a, b) => a - b);
  // Each column at what is picked, the hours at this one if none is; that hour takes the focus.
  const show = (e: Event) => {
    e.preventDefault();
    const centre = (col: HTMLElement, el: HTMLElement) => (col.scrollTop = el.offsetTop - (col.clientHeight - el.offsetHeight) / 2);
    const h = hours.current!.querySelectorAll("button")[hour ?? new Date().getHours()];
    centre(hours.current!, h);
    const m = mins.current!.querySelector<HTMLElement>(".on");
    if (m) centre(mins.current!, m);
    h.focus({ preventScroll: true });
  };
  return (
    <Popover.Root open={open} onOpenChange={setOpen} modal>
      <Popover.Trigger className="row pick">
        <div className="row-inner">
          时间
          <span className="row-value">{value || "不设时间"}</span>
          <Icon of={ChevronsUpDown} size={16} />
        </div>
      </Popover.Trigger>
      <Popover.Portal container={root}>
        <Popover.Content className="menu time-menu" align="end" sideOffset={4} onOpenAutoFocus={show}>
          <div className="time-cols">
            <div ref={hours} role="group" aria-label="时">
              {HOURS.map((h) => (
                <button key={h} className={h === hour ? "on" : undefined} aria-pressed={h === hour} onClick={() => onChange(`${pad(h)}:${pad(minute ?? 0)}`)}>
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
                  onClick={() => (onChange(`${pad(hour!)}:${pad(m)}`), setOpen(false))}
                >
                  {pad(m)}
                </button>
              ))}
            </div>
          </div>
          <button className="menu-item" onClick={() => (onChange(""), setOpen(false))}>
            不设时间
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

const MONTHDAYS = Array.from({ length: 31 }, (_, i) => String(i + 1));

/** The days a repeat falls on, seven a line: Monday 1 to Sunday 7, or the 1st to the 31st. `texts`
 * are on the buttons, `name` says them in full. The last one picked stays. */
function Days(p: { label: string; texts: string[]; name(text: string): string; value: number[]; onChange(days: number[]): void }) {
  return (
    <span className="days" role="group" aria-label={p.label}>
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

const EVERY = { min: 1, max: 365 };

/** How many units apart, `−` and `+` a step, or typed; out of range, the nearest bound. */
function Stepper({ value, onChange }: { value: number; onChange(n: number): void }) {
  // While typing, what is typed, empty say.
  const [typed, setTyped] = useState<string | null>(null);
  return (
    <span className="stepper">
      <Tip label="减少">
        <button disabled={value <= EVERY.min} onClick={() => onChange(value - 1)}>
          <Icon of={Minus} size={16} />
        </button>
      </Tip>
      <input
        inputMode="numeric"
        aria-label="间隔数"
        value={typed ?? value}
        onChange={(e) => {
          const digits = e.target.value.replace(/\D/g, "").slice(0, 3);
          setTyped(digits);
          if (digits) onChange(Math.min(EVERY.max, Math.max(EVERY.min, Number(digits))));
        }}
        onBlur={() => setTyped(null)}
      />
      <Tip label="增加">
        <button disabled={value >= EVERY.max} onClick={() => onChange(value + 1)}>
          <Icon of={Plus} size={16} />
        </button>
      </Tip>
    </span>
  );
}
