// In the Android app: what reminders, the widget and the calendar need from the system, and where to
// grant it.

import { LocalNotifications } from "@capacitor/local-notifications";
import { useEffect, useState } from "react";
import { type CalendarTarget, type DeviceCalendar, NextTodo, setCalendar } from "../android";
import { Select } from "../ui/controls";
import { forUser, useFeedback } from "../ui/feedback";

const TIP_SEEN = "nexttodo.backgroundTip";

interface Allowed {
  notify: boolean;
  exact: boolean;
  calendar: boolean;
}

async function allowed(): Promise<Allowed> {
  const [p, e, c] = await Promise.all([LocalNotifications.checkPermissions(), LocalNotifications.checkExactNotificationSetting(), NextTodo.checkPermissions()]);
  return { notify: p.display === "granted", exact: e.exact_alarm === "granted", calendar: c.calendar === "granted" };
}

/** Asks for notifications. When they stay off, say because the system no longer asks once they
 * were turned off in its settings or turned down for good, opens the app's system settings. */
async function allowNotifications() {
  if ((await LocalNotifications.requestPermissions()).display !== "granted") await NextTodo.openAppSettings();
}

/** The settings section: notifications, exact alarms, autostart, and the system calendar. */
export function BackgroundSettings() {
  const [state, setState] = useState<Allowed | null>(null);
  const check = () => allowed().then(setState);
  useEffect(() => {
    // Back from the system settings, look again.
    const shown = () => document.visibilityState === "visible" && check();
    void check();
    document.addEventListener("visibilitychange", shown);
    return () => document.removeEventListener("visibilitychange", shown);
  }, []);
  if (!state) return null;
  return (
    <>
      <div className="group">
        <Row title="通知" sub={state.notify ? "已允许" : "未允许时不会弹出截止提醒"} action={state.notify ? undefined : ["允许", () => allowNotifications().then(check)]} />
        <Row title="准时提醒" sub={state.exact ? "已允许" : "未允许时提醒可能延迟几分钟"} action={state.exact ? undefined : ["前往设置", () => LocalNotifications.changeExactNotificationSetting()]} />
        <Row title="自启动和省电" sub="澎湃 OS 需允许自启动，并将省电策略设为“无限制”，小组件和提醒才能按时工作" action={["打开设置", () => NextTodo.openAppSettings()]} />
      </div>
      <CalendarSettings allowed={state.calendar} onAllowed={check} />
    </>
  );
}

function Row({ title, sub, action }: { title: string; sub: string; action?: [string, () => unknown] }) {
  return (
    <div className="row">
      <div className="row-inner">
        <span className="folder-main">
          <span>{title}</span>
          <span className="folder-sub">{sub}</span>
        </span>
        {action && (
          <button className="text-btn" onClick={action[1]}>
            {action[0]}
          </button>
        )}
      </div>
    </div>
  );
}

/** Which calendar the tasks with a deadline are written into, if any (docs/android.md#写进系统日历). */
function CalendarSettings({ allowed, onAllowed }: { allowed: boolean; onAllowed(): void }) {
  const { snack } = useFeedback();
  const [target, setTarget] = useState<CalendarTarget | "off" | null>(null);
  const [calendars, setCalendars] = useState<DeviceCalendar[] | null>(null);
  const [denied, setDenied] = useState(false);
  useEffect(() => {
    // Read again once allowed: the calendar written to may have gone, and writing with it.
    NextTodo.calendar().then(
      (r) => setTarget(r.target ?? "off"),
      (e) => snack(forUser("读取日历设置", e)),
    );
    if (allowed)
      NextTodo.calendars().then(
        (r) => setCalendars(r.calendars),
        (e) => snack(forUser("读取手机上的日历", e)),
      );
  }, [allowed, snack]);
  if (target === null) return null;
  const change = (to: CalendarTarget | "off") =>
    setCalendar(to === "off" ? undefined : to).then(
      () => setTarget(to),
      (e) => snack(forUser("设置写进的日历", e)),
    );
  const turnOn = async () => {
    if ((await NextTodo.requestPermissions()).calendar !== "granted") return setDenied(true);
    onAllowed();
    await change("own");
  };
  const settings: [string, () => unknown] = ["前往设置", () => NextTodo.openAppSettings()];
  const chosen = typeof target === "number" ? calendars?.find((c) => c.id === target) : undefined;
  let body;
  if (target === "off") {
    body = denied && !allowed ? <Row title="写进系统日历" sub="未允许读写日历，需在系统设置中允许" action={settings} /> : <Row title="写进系统日历" sub="在系统日历里查看带截止时间的事项" action={["打开", turnOn]} />;
  } else if (!allowed) {
    body = <Row title="写进系统日历" sub="未允许读写日历，日程暂停更新" action={settings} />;
  } else if (calendars) {
    const items: [string, string][] = [["off", "关闭"], ["own", "NextTodo"], ...calendars.map((c): [string, string] => [`${c.id}`, c.name === c.account ? c.name : `${c.name}（${c.account}）`])];
    body = <Select row label="写进系统日历" items={items} value={`${target}`} onChange={(v) => change(v === "off" || v === "own" ? v : Number(v))} />;
  }
  const where = target === "own" ? "写进 App 新建的“NextTodo”日历，只在这台手机上。" : chosen?.synced ? `日程会随账号 ${chosen.account} 同步到云端。` : "这个日历只在这台手机上。";
  return (
    <>
      <h2 className="group-title">系统日历</h2>
      <div className="group">{body}</div>
      {target !== "off" && allowed && calendars && <p className="empty">{where}日程不带提醒，提醒仍由 NextTodo 发出。</p>}
    </>
  );
}

/** Shown once on the first start: autostart is off by default on HyperOS. */
export function BackgroundTip() {
  const [seen, setSeen] = useState(() => localStorage.getItem(TIP_SEEN) !== null);
  if (seen) return null;
  const dismiss = () => {
    localStorage.setItem(TIP_SEEN, "");
    setSeen(true);
  };
  return (
    <div className="tip-card">
      <p>为确保提醒和小组件按时工作，请在系统设置中允许 NextTodo 自启动，并将省电策略设为“无限制”。此项也可在设置中找到。</p>
      <div className="tip-actions">
        <button className="text-btn" onClick={dismiss}>
          关闭
        </button>
        <button
          className="text-btn"
          onClick={() => {
            dismiss();
            void NextTodo.openAppSettings();
          }}
        >
          前往设置
        </button>
      </div>
    </div>
  );
}
