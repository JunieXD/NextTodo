// The settings of reminders every device has (docs/ui.md#设置): the reminders a task gets as it is given
// a due date, and in the apps, which remind, whether reminders hide what they are of, and the daily
// digest.

import { type ReactNode, useState } from "react";
import type { Remind } from "../api/types/Remind";
import { app } from "../api/http";
import { DAY_REMINDERS, MAX_REMINDERS, REMINDERS, inOrder, remindLabel, remindersLabel } from "../domain/dates";
import { setDailyDigest, setDefaultReminders, setHideReminders, useDailyDigest, useDefaultReminders, useHideReminders } from "../remind-settings";
import { MultiSelect, Switch } from "../ui/controls";
import { Modal, useFeedback } from "../ui/feedback";
import { TimeRow } from "./due";
import { useRemindBefore, useRemindOn } from "./remind";

/** The menu of `presets` and those of `value`, the reminders picked, ordered as for a task due at
 * `time`; “自定义…” opens a dialog of the rows of `custom`, which adds its reminder. */
function Pick<R extends Remind>(p: {
  label: string;
  presets: R[];
  value: R[];
  time: string | undefined;
  onChange(v: R[]): void;
  custom: [ReactNode, R];
}) {
  const [adding, setAdding] = useState(false);
  const { snack } = useFeedback();
  const key = (r: Remind) => JSON.stringify(r);
  // A task given a due date gets them all, and can't be saved with more.
  const set = (v: R[]) => (v.length > MAX_REMINDERS ? snack(`提醒最多 ${MAX_REMINDERS} 个`) : p.onChange(v));
  const value = inOrder(p.value, p.time);
  // From the due time back.
  const listed = inOrder([...p.presets, ...value], p.time).reverse();
  return (
    <>
      <MultiSelect
        label={p.label}
        text={remindersLabel(value.map(remindLabel))}
        groups={[{ options: listed.map((r) => ({ value: key(r), text: remindLabel(r) })) }]}
        value={value.map(key)}
        onChange={(keys) => set(listed.filter((r) => keys.includes(key(r))))}
        none="不提醒"
        more={["自定义…", () => setAdding(true)]}
      />
      {adding && (
        <Modal
          title="自定义提醒"
          onClose={() => setAdding(false)}
          footer={
            <>
              <button className="text-btn" onClick={() => setAdding(false)}>
                取消
              </button>
              <button className="filled-btn" onClick={() => (set(inOrder([...value, p.custom[1]], p.time)), setAdding(false))}>
                添加
              </button>
            </>
          }
        >
          <div className="group settings">{p.custom[0]}</div>
        </Modal>
      )}
    </>
  );
}

export function RemindSettings() {
  const defaults = useDefaultReminders();
  const before = useRemindBefore();
  const on = useRemindOn();
  const hidden = useHideReminders();
  const digest = useDailyDigest();
  return (
    <>
      <div className="group settings">
        {/* Only so far before the time: one by the day, 当天 09:00 say, would come after a task due
            earlier. Any time orders those alike. */}
        <Pick
          label="有时间的事项"
          presets={REMINDERS.map((before) => ({ before }))}
          value={defaults.timed}
          time="00:00"
          onChange={(timed) => setDefaultReminders({ ...defaults, timed })}
          custom={before}
        />
        <Pick
          label="只有日期的事项"
          presets={DAY_REMINDERS}
          value={defaults.dated}
          time={undefined}
          onChange={(dated) => setDefaultReminders({ ...defaults, dated })}
          custom={on}
        />
      </div>
      {app && (
        <>
          <div className="group settings">
            <Switch label="隐藏通知内容" sub="通知里不显示事项的标题和文件夹" on={hidden} onChange={setHideReminders} />
          </div>
          <div className="group settings">
            <Switch
              label="每日汇总"
              sub="每天定时通知今天截止和已逾期的未完成事项"
              on={digest.on}
              onChange={(on) => setDailyDigest({ ...digest, on })}
            />
            {digest.on && <TimeRow value={digest.time} onChange={(time) => setDailyDigest({ ...digest, time })} />}
          </div>
        </>
      )}
    </>
  );
}
