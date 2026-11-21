// The settings of reminders every device has (docs/ui.md#设置): the reminders a task gets as it is given
// a due date, and in the apps, which remind, whether reminders hide what they are of.

import type { Remind } from "../api/types/Remind";
import { app } from "../api/http";
import { DAY_REMINDERS, REMINDERS, inOrder, remindLabel, remindersLabel } from "../domain/dates";
import { setDefaultReminders, setHideReminders, useDefaultReminders, useHideReminders } from "../remind-settings";
import { MultiSelect, Switch } from "../ui/controls";

/** `offered` as the menu lists them; `value`, those picked, in the order they come. */
function Pick<R extends Remind>({ label, offered, value, onChange }: { label: string; offered: R[]; value: Remind[]; onChange(v: R[]): void }) {
  const key = (r: Remind) => JSON.stringify(r);
  return (
    <MultiSelect
      label={label}
      text={remindersLabel(value)}
      items={offered.map((r) => [key(r), remindLabel(r)])}
      value={value.map(key)}
      onChange={(keys) => onChange(offered.filter((r) => keys.includes(key(r))))}
      none="不提醒"
    />
  );
}

export function RemindSettings() {
  const defaults = useDefaultReminders();
  const hidden = useHideReminders();
  const timed = REMINDERS.map((before) => ({ before }));
  return (
    <>
      <div className="group settings">
        {/* Any time orders those before it alike. */}
        <Pick label="有时间的事项" offered={timed} value={inOrder(defaults.timed, "00:00")} onChange={(v) => setDefaultReminders({ ...defaults, timed: v })} />
        <Pick
          label="只有日期的事项"
          offered={DAY_REMINDERS}
          value={inOrder(defaults.dated, undefined)}
          onChange={(v) => setDefaultReminders({ ...defaults, dated: v })}
        />
      </div>
      {app && (
        <div className="group settings">
          <Switch label="隐藏通知内容" sub="通知只写“有事项到期”，不显示标题和文件夹" on={hidden} onChange={setHideReminders} />
        </div>
      )}
    </>
  );
}
