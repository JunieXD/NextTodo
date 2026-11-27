// A task's reminders in its detail (docs/ui/dialogs.md): the row saying them all, its menu ticking them by
// what they go by, and the dialog adding one not on it; and the rows choosing a reminder, which the
// settings of default reminders use too.

import { Bell } from "lucide-react";
import { type ReactNode, useState } from "react";
import type { Remind } from "../api/types/Remind";
import type { RemindAt } from "../api/types/RemindAt";
import type { RemindBefore } from "../api/types/RemindBefore";
import type { RemindOn } from "../api/types/RemindOn";
import type { Task } from "../api/types/Task";
import {
  DAY_REMINDERS,
  MAX_REMIND,
  MAX_REMINDERS,
  MAX_REMIND_DAYS,
  REMINDERS,
  REMIND_UNITS,
  type RemindUnit,
  inOrder,
  isoDate,
  remindAtLabel,
  remindAtTime,
  remindLabel,
  remindersLabel,
  startInOrder,
  startRemindLabel,
  timesInOrder,
} from "../domain/dates";
import type { TaskFields } from "../domain/apply";
import { reminderLabels } from "../domain/reminders";
import { MultiSelect, Seg, Stepper } from "../ui/controls";
import { Modal, useFeedback } from "../ui/feedback";
import { Calendar, Controls, TimeRow } from "./due";

/** A task's reminders of each kind, as they are kept (`inOrder`, `startInOrder`, `timesInOrder`). */
interface Picked {
  due: Remind[];
  start: Remind[];
  at: RemindAt[];
}

/** A reminder added, by what it goes by: the due date, the start, or a time of its own. */
type Added = { by: "due" | "start"; r: Remind } | { by: "at"; r: RemindAt };

const AHEAD = (["minute", "hour", "day"] as const).map((u): [RemindUnit, string] => [u, REMIND_UNITS[u].name]);
const remindKey = (r: Remind) => ("before" in r ? `${r.before}` : `${r.days} ${r.time}`);
const atKey = (r: RemindAt) => `at ${r.date} ${r.time}`;
const without = <T extends { reminders?: Remind[] }>({ reminders: _, ...rest }: T) => rest;

/** The row of a task's reminders, of all kinds, ticked in its menu and saved at once by `update`. */
export function ReminderRow({ task: t, now, update }: { task: Task; now: Date; update(fields: TaskFields): void }) {
  const { snack } = useFeedback();
  const [adding, setAdding] = useState(false);
  const timing = t.due_after ?? t.due;
  const have: Picked = { due: timing?.reminders ?? [], start: t.start_reminders ?? [], at: t.remind_at ?? [] };
  // Those by a due date or the start that the menu offers, with those it has, from the time back.
  const offered = (time: string | undefined, its: Remind[]) => inOrder([...(time ? REMINDERS.map((before) => ({ before })) : DAY_REMINDERS), ...its], time).reverse();
  const due = (timing ? offered(timing.time, have.due) : []).map((r) => ({ value: `due ${remindKey(r)}`, text: remindLabel(r), r }));
  const start = (t.start ? offered(t.start_time, have.start) : []).map((r) => ({ value: `start ${remindKey(r)}`, text: startRemindLabel(r), r }));
  const at = have.at.map((r) => ({ value: atKey(r), text: remindAtLabel(r, now), r, ...(remindAtTime(r) < now && { note: "已过" }) }));
  const groups = [
    ...(timing ? [{ label: "截止", options: due }] : []),
    ...(t.start ? [{ label: "开始", options: start }] : []),
    ...(at.length ? [{ label: "指定时间", options: at }] : []),
  ];
  const save = (to: Picked) => {
    if (to.due.length + to.start.length + to.at.length > MAX_REMINDERS) return snack(`提醒最多 ${MAX_REMINDERS} 个`);
    const changed = <T,>(a: T[], b: T[]) => JSON.stringify(a) !== JSON.stringify(b);
    const fields: TaskFields = {};
    if (changed(to.due, have.due)) {
      const reminders = to.due.length ? { reminders: to.due } : {};
      if (t.due_after) fields.due_after = { ...without(t.due_after), ...reminders };
      else if (t.due) fields.due = { ...without(t.due), ...reminders };
    }
    if (changed(to.start, have.start)) fields.start_reminders = to.start;
    if (changed(to.at, have.at)) fields.remind_at = to.at;
    if (Object.keys(fields).length) update(fields);
  };
  const pick = (values: string[]) => {
    const of = <R,>(options: { value: string; r: R }[]) => options.filter((o) => values.includes(o.value)).map((o) => o.r);
    save({ due: inOrder(of(due), timing?.time), start: startInOrder(of(start)), at: of(at) });
  };
  const add = (a: Added) =>
    save(
      a.by === "at"
        ? { ...have, at: timesInOrder([...have.at, a.r]) }
        : a.by === "start"
          ? { ...have, start: startInOrder([...have.start, a.r]) }
          : { ...have, due: inOrder([...have.due, a.r], timing?.time) },
    );
  return (
    <>
      <MultiSelect
        label="提醒"
        icon={Bell}
        text={remindersLabel(reminderLabels(t, now))}
        groups={groups}
        value={[...have.due.map((r) => `due ${remindKey(r)}`), ...have.start.map((r) => `start ${remindKey(r)}`), ...have.at.map(atKey)]}
        onChange={pick}
        none="不提醒"
        more={["自定义…", () => setAdding(true)]}
      />
      {adding && (
        <CustomReminder
          kinds={[...(timing ? [{ by: "due" as const, timed: !!timing.time }] : []), ...(t.start ? [{ by: "start" as const, timed: !!t.start_time }] : [])]}
          now={now}
          onAdd={add}
          onClose={() => setAdding(false)}
        />
      )}
    </>
  );
}

const NAMES = { due: "截止时间", start: "开始时间", at: "指定时间" };

/** “自定义提醒”: one reminder by what the task has of `kinds`, with a time or not, or at a time of its own. */
function CustomReminder({
  kinds,
  now,
  onAdd,
  onClose,
}: {
  kinds: { by: "due" | "start"; timed: boolean }[];
  now: Date;
  onAdd(added: Added): void;
  onClose(): void;
}) {
  const [by, setBy] = useState<Added["by"]>(kinds[0]?.by ?? "at");
  const [byDay, setByDay] = useState(false);
  const [beforeRows, before] = useRemindBefore();
  const [onDayRows, onDay] = useRemindOn();
  const [date, setDate] = useState(() => isoDate(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1)));
  const [time, setTime] = useState("09:00");
  // A time of its own refused as past when adding it, by the clock then, until another is picked.
  const [past, setPast] = useState<string | null>(null);
  const timed = kinds.find((k) => k.by === by)?.timed ?? false;
  const ahead = timed && !byDay;
  const add = () => {
    if (by === "at" && remindAtTime({ date, time }) < new Date()) return setPast(`${date} ${time}`);
    onAdd(by === "at" ? { by, r: { date, time } } : { by, r: ahead ? before : onDay });
    onClose();
  };
  return (
    <Modal
      title="自定义提醒"
      onClose={onClose}
      footer={
        <>
          {by === "at" && past === `${date} ${time}` && (
            <p className="form-error" role="alert">
              这个时间已经过了
            </p>
          )}
          <button className="text-btn" onClick={onClose}>
            取消
          </button>
          <button className="filled-btn" onClick={add}>
            添加
          </button>
        </>
      }
    >
      {kinds.length > 0 && (
        <div className="due-mode">
          <Seg<Added["by"]> label="按什么时间" items={[...kinds.map((k): [Added["by"], string] => [k.by, NAMES[k.by]]), ["at", NAMES.at]]} value={by} onChange={setBy} />
        </div>
      )}
      {by === "at" ? (
        <>
          <Calendar value={date} min={isoDate(now)} onChange={setDate} />
          <div className="group settings">
            <TimeRow label="几点" value={time} onChange={setTime} />
          </div>
        </>
      ) : (
        <>
          {timed && (
            <div className="due-mode">
              <Seg<"ahead" | "day">
                label="提醒方式"
                items={[
                  ["ahead", "提前多久"],
                  ["day", "按日子"],
                ]}
                value={byDay ? "day" : "ahead"}
                onChange={(k) => setByDay(k === "day")}
              />
            </div>
          )}
          <div className="group settings">{ahead ? beforeRows : onDayRows}</div>
        </>
      )}
    </Modal>
  );
}

/** The rows choosing a reminder so far ahead of the time, and that reminder. */
export function useRemindBefore(): [ReactNode, RemindBefore] {
  const [ahead, setAhead] = useState<[number, RemindUnit]>([10, "minute"]);
  const rows = (
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
  );
  return [rows, { before: ahead[0] * REMIND_UNITS[ahead[1]].minutes }];
}

/** The rows choosing a reminder on the day, or days before it, at a time, and that reminder. */
export function useRemindOn(): [ReactNode, RemindOn] {
  const [onDay, setOnDay] = useState<[number, string]>([1, "09:00"]);
  const rows = (
    <>
      <Controls name="提前几天">
        <Stepper label="提前几天" min={0} value={onDay[0]} max={MAX_REMIND_DAYS} onChange={(n) => setOnDay([n, onDay[1]])} />
      </Controls>
      <TimeRow label="几点" value={onDay[1]} onChange={(t) => setOnDay([onDay[0], t])} />
    </>
  );
  return [rows, { days: onDay[0], time: onDay[1] }];
}
