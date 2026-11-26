// In the desktop app: starting at login, in the menu bar or tray only, and the panel's shortcut.

import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { X } from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import { desktopOs } from "../api/http";
import { type Shortcut, shortcut } from "../desktop";
import { Switch } from "../ui/controls";
import { Tip, forUser, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";

const windows = desktopOs === "Windows";

export function DesktopSystem() {
  return (
    <div className="group settings">
      <StartAtLogin />
      <PanelShortcut />
    </div>
  );
}

function StartAtLogin() {
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
  return <Switch label="登录时启动" sub={`登录电脑后自动启动，只显示在${windows ? "系统托盘" : "菜单栏"}`} on={on} onChange={toggle} />;
}

/** The modifier keys, as the app's shortcuts name them, in the order the system writes them. */
const MODIFIERS = ["control", "alt", "shift", "super"] as const;
type Modifier = (typeof MODIFIERS)[number];
const SIGNS: Record<Modifier, string> = windows
  ? { control: "Ctrl", alt: "Alt", shift: "Shift", super: "Win" }
  : { control: "⌃", alt: "⌥", shift: "⇧", super: "⌘" };

/** Keys whose `KeyboardEvent.code` is not what is written on them. */
const KEYS: Record<string, string> = {
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backquote: "`",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

/** Pressed on their own, they only show what is held. */
const HELD = ["Control", "Alt", "AltGraph", "Shift", "Meta", "CapsLock", "Fn"];

/** As the system writes a shortcut, ⌃⌥K on macOS and Ctrl+Alt+K on Windows: of `keys`, the modifiers
 * among them and `key`, by its `KeyboardEvent.code`; without one yet, what is held so far. */
function written(keys: readonly string[], key?: string) {
  const last = key === undefined ? "…" : (KEYS[key] ?? key.replace(/^(Key|Digit)/, ""));
  return [...MODIFIERS.filter((m) => keys.includes(m)).map((m) => SIGNS[m]), last].join(windows ? "+" : "");
}

const held = (e: KeyboardEvent) => MODIFIERS.filter((m) => ({ control: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, super: e.metaKey })[m]);

/** The panel's global shortcut: none until picked here, by pressing it in the box. Esc, leaving the
 * box or the window, ends taking one (docs/desktop.md#小窗快捷键). */
function PanelShortcut() {
  const { snack } = useFeedback();
  const [saved, setSaved] = useState<Shortcut | null>();
  // While taking a new one: the modifiers held, and whether a key came without those that count.
  const [taking, setTaking] = useState<{ held: Modifier[]; bare: boolean } | null>(null);
  const box = useRef<HTMLButtonElement>(null);
  // One that can't be read shows as none, for a new one to take its place.
  const load = useCallback(() => shortcut.get().then(setSaved, (e) => (setSaved(null), snack(forUser("读取小窗快捷键", e)))), [snack]);
  useEffect(() => void load(), [load]);
  const on = taking !== null;
  // Stopped while taking one, so that pressing the same keys does not open the panel; started again
  // however that ends, the page going too.
  useEffect(() => {
    if (!on) return;
    shortcut.pause(true).catch((e) => snack(forUser("停用小窗快捷键", e)));
    const stop = () => setTaking(null);
    addEventListener("blur", stop);
    return () => {
      removeEventListener("blur", stop);
      shortcut.pause(false).then(load, (e) => snack(forUser("恢复小窗快捷键", e)));
    };
  }, [on, load, snack]);
  if (saved === undefined) return null;
  const start = () => {
    if (on) return;
    setTaking({ held: [], bare: false });
    // WebKit leaves a clicked button unfocused.
    box.current?.focus();
  };
  const press = (e: KeyboardEvent) => {
    if (!taking) return;
    e.preventDefault();
    const now = held(e);
    if (e.key === "Escape" && !now.length) setTaking(null);
    else if (HELD.includes(e.key)) setTaking({ held: now, bare: false });
    else if (!e.ctrlKey && !e.altKey && !e.metaKey) setTaking({ held: now, bare: true });
    else
      shortcut.set([...now, e.code].join("+")).then(
        // Out of the box: the Space of ⌃⌥Space, let go there, would click it and take one again.
        () => box.current?.blur(),
        (e) => snack(forUser("设置小窗快捷键", e)),
      );
  };
  const clear = () => shortcut.set(null).then(load, (e) => snack(forUser("清除小窗快捷键", e)));
  const keys = saved?.keys.split("+");
  const sub = taking
    ? taking.bare
      ? `要带上 ${windows ? "Ctrl、Alt 或 Win" : "⌘、⌃ 或 ⌥"}`
      : "按 Esc 取消"
    : saved?.taken
      ? "未生效，可能已被其他应用占用"
      : "在任何应用里按下，弹出或收起小窗";
  return (
    <div className="row">
      <div className="row-inner">
        <span className="folder-main">
          <span>小窗快捷键</span>
          <span className="folder-sub full">{sub}</span>
        </span>
        <button
          ref={box}
          className={taking ? "keys taking" : keys ? "keys" : "keys none"}
          onClick={start}
          onKeyDown={press}
          onKeyUp={(e) => taking && setTaking({ ...taking, held: held(e) })}
          onBlur={() => setTaking(null)}
        >
          {taking ? (taking.held.length ? written(taking.held) : "请按下快捷键") : keys ? written(keys, keys.at(-1)) : "未设置"}
        </button>
        {keys && (
          // Hidden rather than gone while taking one, so that the box stays where it was clicked.
          <Tip label="清除快捷键">
            <button className="icon-btn sm" onClick={clear} style={taking ? { visibility: "hidden" } : undefined}>
              <Icon of={X} size={16} />
            </button>
          </Tip>
        )}
      </div>
    </div>
  );
}
