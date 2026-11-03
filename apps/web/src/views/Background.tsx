// In the Android app: what reminders and the widget need from the system, and where to grant it.

import { LocalNotifications } from "@capacitor/local-notifications";
import { useEffect, useState } from "react";
import { NextTodo } from "../native";

const TIP_SEEN = "nexttodo.backgroundTip";

interface Allowed {
  notify: boolean;
  exact: boolean;
}

async function allowed(): Promise<Allowed> {
  const [p, e] = await Promise.all([LocalNotifications.checkPermissions(), LocalNotifications.checkExactNotificationSetting()]);
  return { notify: p.display === "granted", exact: e.exact_alarm === "granted" };
}

/** Asks for notifications. When they stay off, say because the system no longer asks once they
 * were turned off in its settings or turned down for good, opens the app's system settings. */
async function allowNotifications() {
  if ((await LocalNotifications.requestPermissions()).display !== "granted") await NextTodo.openAppSettings();
}

/** The settings section: notifications, exact alarms, and autostart. */
export function BackgroundSettings() {
  const [state, setState] = useState<Allowed | null>(null);
  useEffect(() => {
    // Back from the system settings, look again.
    const check = () => document.visibilityState === "visible" && allowed().then(setState);
    check();
    document.addEventListener("visibilitychange", check);
    return () => document.removeEventListener("visibilitychange", check);
  }, []);
  if (!state) return null;
  const row = (title: string, sub: string, action?: [string, () => unknown]) => (
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
  return (
    <>
      <h2 className="group-title">提醒和后台</h2>
      <div className="group">
        {row("通知", state.notify ? "已允许" : "不允许时，截止提醒不会弹出", state.notify ? undefined : ["允许", () => allowNotifications().then(allowed).then(setState)])}
        {row("准时提醒", state.exact ? "已允许" : "不允许时，提醒可能晚几分钟", state.exact ? undefined : ["去设置", () => LocalNotifications.changeExactNotificationSetting()])}
        {row("自启动和省电", "澎湃 OS 上请允许自启动，省电策略选“无限制”，小组件和提醒才能按时工作", ["打开设置", () => NextTodo.openAppSettings()])}
      </div>
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
      <p>让提醒和小组件按时工作：在系统设置里允许 NextTodo 自启动，省电策略选“无限制”。以后也能在设置里找到。</p>
      <div className="tip-actions">
        <button className="text-btn" onClick={dismiss}>
          知道了
        </button>
        <button
          className="text-btn"
          onClick={() => {
            dismiss();
            void NextTodo.openAppSettings();
          }}
        >
          去设置
        </button>
      </div>
    </div>
  );
}
