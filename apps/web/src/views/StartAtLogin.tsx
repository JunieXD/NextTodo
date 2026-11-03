// In the macOS app: starting at login, in the menu bar only.

import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useEffect, useState } from "react";
import { useFeedback } from "../ui/feedback";

export function StartAtLogin() {
  const { snack } = useFeedback();
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    isEnabled().then(setOn, (e) => snack(`读取开机自启设置失败：${e}`));
  }, [snack]);
  if (on === null) return null;
  const toggle = async () => {
    try {
      await (on ? disable() : enable());
      setOn(await isEnabled());
    } catch (e) {
      snack(`${on ? "关闭" : "开启"}开机自启失败：${e}`);
    }
  };
  return (
    <>
      <h2 className="group-title">开机自启</h2>
      <div className="group">
        <div className="row">
          <div className="row-inner">
            <span className="folder-main">
              <span>登录时启动</span>
              <span className="folder-sub">{on ? "已开启：启动后仅显示在菜单栏，提醒照常" : "开启后登录时自动启动，仅显示在菜单栏"}</span>
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
