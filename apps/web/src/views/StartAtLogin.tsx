// In the desktop app: starting at login, in the menu bar or tray only.

import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useEffect, useState } from "react";
import { desktopOs } from "../api/http";
import { Switch } from "../ui/controls";
import { forUser, useFeedback } from "../ui/feedback";

const TRAY = desktopOs === "Windows" ? "系统托盘" : "菜单栏";

export function StartAtLogin() {
  const { snack } = useFeedback();
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    isEnabled().then(setOn, (e) => snack(forUser("读取开机自启设置", e)));
  }, [snack]);
  if (on === null) return null;
  const toggle = async (to: boolean) => {
    try {
      await (to ? enable() : disable());
      setOn(await isEnabled());
    } catch (e) {
      snack(forUser(`${to ? "开启" : "关闭"}开机自启`, e));
    }
  };
  return (
    <div className="group">
      <Switch label="登录时启动" sub={`登录电脑后自动启动，只显示在${TRAY}，提醒照常`} on={on} onChange={toggle} />
    </div>
  );
}
