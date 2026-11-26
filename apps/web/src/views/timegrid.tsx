// The calendar's time grid (docs/ui.md#日历): a week by the hour, what is due in half-hour cells and
// what is due on a day alone above them; a tap on an empty cell picks it to add to.

import { Plus } from "lucide-react";
import { type ComponentProps, type CSSProperties, useLayoutEffect, useRef, useState } from "react";
import { completionKey } from "../domain/apply";
import { lateness } from "../domain/calendar";
import { day, isoTime } from "../domain/dates";
import { type Block, fit, firstLook, opensAt, slotOf, slotTime, weekBlocks } from "../domain/timegrid";
import { useView } from "../session";
import { Modal } from "../ui/feedback";
import { useWide } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { ContextMenu } from "../ui/menu";
import { useTaskActions } from "./actions";
import { DayNote, daySpoken, useHolidayMarks } from "./daynote";
import { WEEKDAYS } from "./due";
import { useSelection } from "./nav";
import { Occurrence, type RowActions, TaskRow, useTaskMenu } from "./rows";

/** How narrow a block may be beside others in its cell, and the room around each. */
const MIN_BLOCK = 50;
const AROUND = 4;

/** Hour labels this close to now, in minutes, make way for its time. */
const NEAR_NOW = 20;

/** A block's key among a day's. */
export const keyOf = (b: Block) => (b.record ? `record ${completionKey(b.record)}` : `${b.later ? "later" : "task"} ${b.task.id}`);

/** Where a block goes in its half hour: `i`th of `k` side by side. */
const place = (slot: number, i: number, k: number): CSSProperties => ({
  top: `calc(${slot} * var(--hour) / 2 + 1px)`,
  height: "calc(var(--hour) / 2 - 2px)",
  left: `calc(${i} * 100% / ${k} + ${AROUND / 2}px)`,
  width: `calc(100% / ${k} - ${AROUND}px)`,
});

/** `11月25日 周三` */
const dayName = (date: string) => {
  const d = day(date);
  return `${d.getMonth() + 1}月${d.getDate()}日 周${WEEKDAYS[(d.getDay() + 6) % 7]}`;
};

/** The week of `days`, `picked` among them; `at`, the half hour picked on it to add to. `onPick` picks a
 * day, with a time the half hour from it, or none. */
export function TimeGrid({
  days,
  today,
  picked,
  at,
  now,
  onPick,
}: {
  days: string[];
  today: string;
  picked: string;
  at: string | null;
  now: Date;
  onPick(date: string, time?: string): void;
}) {
  const { board } = useView();
  const wide = useWide();
  const [marks] = useHolidayMarks();
  const actions = useTaskActions();
  const { selected } = useSelection();
  const week = weekBlocks(board, days, now);
  const [cell, setCell] = useState<{ date: string; slot: number } | null>(null);
  const [allDay, setAllDay] = useState(false);
  // How many blocks a cell has room for side by side.
  const cols = useRef<HTMLDivElement>(null);
  const [room, setRoom] = useState(1);
  useLayoutEffect(() => {
    const el = cols.current!;
    const measured = new ResizeObserver(() => setRoom(Math.max(1, Math.floor(el.clientWidth / 7 / (MIN_BLOCK + AROUND)))));
    measured.observe(el);
    return () => measured.disconnect();
  }, []);
  // Opened, and on to another week, at a time worth seeing, below the days that stay on top.
  const head = useRef<HTMLDivElement>(null);
  const mark = useRef<HTMLDivElement>(null);
  const first = days[0];
  useLayoutEffect(() => {
    // Below the days and what stays on top over them.
    mark.current!.style.scrollMarginTop = `${head.current!.offsetHeight + parseFloat(getComputedStyle(head.current!).top)}px`;
    mark.current!.scrollIntoView({ block: "start" });
  }, [first]);
  const minutes = now.getHours() * 60 + now.getMinutes();
  const thisWeek = days.includes(today);
  const fits = wide ? 3 : 2;
  // Opening a task from the panel closes it.
  const fromPanel: RowActions = { ...actions, open: (t) => (setCell(null), actions.open(t)) };
  // Gone with the week it is in, or once nothing is left in it.
  const inCell = cell && week.get(cell.date)?.filter((b) => b.time && slotOf(b.time) === cell.slot);
  if (cell && !inCell?.length) setCell(null);
  // A block's menu and the picker it opens send their clicks to the day too, by React's tree.
  const onDay = (e: React.MouseEvent) => e.currentTarget.contains(e.target as Node);

  const blockButton = (b: Block, style?: CSSProperties) => <DayBlock key={keyOf(b)} block={b} now={now} actions={actions} style={style} />;
  return (
    <div className="tg">
      <div ref={head} className="tg-head">
        <div className="tg-row">
          <span />
          {days.map((d, i) => (
            <button
              key={d}
              type="button"
              className="tg-day"
              aria-pressed={d === picked}
              aria-current={d === today ? "date" : undefined}
              aria-label={`星期${WEEKDAYS[i]}，${Number(d.slice(5, 7))}月${Number(d.slice(8))}日${daySpoken(d, marks)}`}
              onClick={() => onPick(d)}
            >
              <small>{d === today ? "今" : WEEKDAYS[i]}</small>
              <span className="cal-num">{Number(d.slice(8))}</span>
              <DayNote date={d} marks={marks} />
            </button>
          ))}
        </div>
        <div className="tg-row tg-allday">
          <span className="tg-gutter">全天</span>
          {days.map((d) => {
            const list = week.get(d)!.filter((b) => !b.time);
            const more = list.length > fits;
            return (
              <div key={d} className="tg-cell" onClick={(e) => onDay(e) && onPick(d)}>
                {(more && !allDay ? list.slice(0, fits - 1) : list).map((b) => blockButton(b))}
                {more && (
                  <button type="button" className="tg-label more" onClick={(e) => (e.stopPropagation(), setAllDay(!allDay))}>
                    {allDay ? "收起" : `+${list.length - fits + 1}`}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      </div>
      <div className="tg-body">
        <div className="tg-hours" aria-hidden>
          {Array.from({ length: 23 }, (_, i) => i + 1)
            .filter((h) => !thisWeek || Math.abs(h * 60 - minutes) >= NEAR_NOW)
            .map((h) => (
              <span key={h} style={{ top: `calc(${h} * var(--hour))` }}>
                {h}:00
              </span>
            ))}
          {thisWeek && (
            <span className="now" style={{ top: `calc(${minutes} / 60 * var(--hour))` }}>
              {isoTime(now)}
            </span>
          )}
        </div>
        <div ref={cols} className="tg-cols">
          {thisWeek && <div className="tg-now" style={{ top: `calc(${minutes} / 60 * var(--hour))` }} />}
          {days.map((d) => {
            const bySlot = new Map<number, Block[]>();
            for (const b of week.get(d)!) if (b.time) bySlot.set(slotOf(b.time), [...(bySlot.get(slotOf(b.time)) ?? []), b]);
            const pick = (e: React.MouseEvent<HTMLDivElement>) => {
              if (!onDay(e)) return;
              const r = e.currentTarget.getBoundingClientRect();
              onPick(d, slotTime(Math.min(47, Math.floor(((e.clientY - r.top) / r.height) * 48))));
            };
            return (
              <div key={d} className={`tg-col${d === today ? " today" : ""}`} onClick={pick}>
                {d === today && <div className="tg-now-today" style={{ top: `calc(${minutes} / 60 * var(--hour))` }} />}
                {at && d === picked && (
                  <button type="button" className="tg-new" aria-label="取消选中的时间" style={place(slotOf(at), 0, 1)} onClick={(e) => (e.stopPropagation(), onPick(d))}>
                    <Icon of={Plus} size={16} />
                  </button>
                )}
                {[...bySlot].flatMap(([slot, list]) => {
                  const { shown, merged } = fit(list, room);
                  const k = shown.length + (merged.length ? 1 : 0);
                  return [
                    ...shown.map((b, i) => blockButton(b, place(slot, i, k))),
                    ...(merged.length
                      ? [
                          <button
                            key={`merged ${slot}`}
                            type="button"
                            className={`tg-block merged ${firstLook(merged)}`}
                            style={place(slot, k - 1, k)}
                            onClick={(e) => (e.stopPropagation(), setCell({ date: d, slot }))}
                          >
                            <span>{shown.length ? `+${merged.length}` : `${merged.length} 项`}</span>
                          </button>,
                        ]
                      : []),
                  ];
                })}
              </div>
            );
          })}
          <div ref={mark} className="tg-mark" style={{ top: `calc(${opensAt(week, now, today)} / 60 * var(--hour))` }} />
        </div>
      </div>
      {cell && inCell && (
        <Modal title={`${dayName(cell.date)} ${slotTime(cell.slot)}–${cell.slot === 47 ? "24:00" : slotTime(cell.slot + 1)}`} onClose={() => setCell(null)}>
          <div className="group">
            {inCell.map((b) =>
              b.later ? (
                <Occurrence key={keyOf(b)} task={b.task} meta={`${b.time} · 以后的一次`} onOpen={() => fromPanel.open(b.task)} />
              ) : b.record ? (
                <Occurrence key={keyOf(b)} done task={b.task} meta={`${b.time} · 完成一次 · ${lateness(b.record)}`} onOpen={() => fromPanel.open(b.task)} />
              ) : (
                <TaskRow key={keyOf(b)} task={b.task} board={board} now={now} path selected={selected === b.task.id} actions={fromPanel} />
              ),
            )}
          </div>
        </Modal>
      )}
    </div>
  );
}

/** What is on a day, which opens its task: a block at its time, or a `label`, for one due on the day
 * alone or in the month with titles. That of a task as it is now has the task's menu on a right click. */
export function DayBlock({
  block,
  now,
  actions,
  label,
  ...rest
}: { block: Block; now: Date; actions: RowActions; label?: boolean } & Pick<ComponentProps<"button">, "style" | "tabIndex">) {
  const button = <BlockButton {...rest} block={block} label={label || !block.time} onOpen={() => actions.open(block.task)} />;
  return block.later || block.record ? button : <TaskMenu block={block} now={now} actions={actions}>{button}</TaskMenu>;
}

/** A block or a label; `rest`, where it goes and what a menu around it adds. */
function BlockButton({ block, label, onOpen, ...rest }: { block: Block; label: boolean; onOpen(): void } & ComponentProps<"button">) {
  return (
    <button {...rest} type="button" className={`${label ? "tg-label" : "tg-block"} ${block.look}`} title={block.task.title} onClick={(e) => (e.stopPropagation(), onOpen())}>
      <span>{block.task.title}</span>
    </button>
  );
}

/** The task's menu around `children`, on a right click. */
function TaskMenu({ block, now, actions, children }: { block: Block; now: Date; actions: RowActions; children: React.ReactElement }) {
  const { menu, picker } = useTaskMenu(block.task, now, actions, {});
  return (
    <>
      <ContextMenu items={menu}>{children}</ContextMenu>
      {picker}
    </>
  );
}
