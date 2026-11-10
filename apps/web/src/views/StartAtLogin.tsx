// In the desktop app: starting at login, in the menu bar or tray only.

import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useEffect, useState } from "react";
import { desktopOs } from "../api/http";
import { forUser, useFeedback } from "../ui/feedback";

const TRAY = desktopOs === "Windows" ? "系统托盘" : "菜单栏";

export function StartAtLogin() {
  const { snack } = useFeedback();
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    isEnabled().then(setOn, (e) => snack(forUser("读取开机自启设置", e)));
  }, [snack]);
  if (on === null) return null;
  const toggle = async () => {
    try {
      await (on ? disable() : enable());
      setOn(await isEnabled());
    } catch (e) {
      snack(forUser(`${on ? "关闭" : "开启"}开机自启`, e));
    }
  };
  return (
    <>
      <div className="group">
        <div className="row">
          <div className="row-inner">
            <span className="folder-main">
              <span>登录时启动</span>
              <span className="folder-sub">{on ? `已开启：启动后仅显示在${TRAY}，提醒照常` : `开启后登录时自动启动，仅显示在${TRAY}`}</span>
            </span>
            <button className="text-btn" onClick={toggle}>
              {on ? "关闭" : "开启"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
