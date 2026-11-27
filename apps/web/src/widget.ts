// How the desktop widget looks (docs/desktop.md#桌面小组件): the background, opacity and text colour of
// the Android widgets (domain/widget-look.ts), kept on this computer, and their colours on its page.
// Its text size is its zoom (zoom.ts).

import { useSyncExternalStore } from "react";
import { type WidgetLook, widgetColors } from "./domain/widget-look";
import { isDark, onThemeChange } from "./theme";

export type DesktopLook = Pick<WidgetLook, "background" | "opacity" | "text">;

const KEY = "nexttodo.widgetLook";

/** See-through for the most part, as it is wanted on the desktop. */
const DEFAULT: DesktopLook = { background: "system", opacity: 70, text: "auto" };

const listeners = new Set<() => void>();
const changed = () => {
  for (const l of listeners) l();
};
// Chosen in the main window's settings, for the widget's.
addEventListener("storage", (e) => e.key === KEY && changed());

const onLook = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

/** The same object until it changes, as React wants of what it subscribes to. */
let last: { saved: string | null; look: DesktopLook } | undefined;
export function desktopLook(): DesktopLook {
  const saved = localStorage.getItem(KEY);
  if (last?.saved !== saved) last = { saved, look: saved === null ? DEFAULT : { ...DEFAULT, ...(JSON.parse(saved) as Partial<DesktopLook>) } };
  return last.look;
}

export function setDesktopLook(look: DesktopLook) {
  localStorage.setItem(KEY, JSON.stringify(look));
  changed();
}

export const useDesktopLook = () => useSyncExternalStore(onLook, desktopLook);

/** In the widget: its colours on the page, for styles/desktop.css, as chosen and as the theme is light or dark;
 * again whenever either changes. */
export function paintWidget() {
  const paint = () => {
    const c = widgetColors(desktopLook(), isDark());
    const root = document.documentElement.style;
    root.setProperty("--w-bg", c.background);
    root.setProperty("--w-text", c.text);
    root.setProperty("--w-secondary", c.secondary);
    root.setProperty("--w-done", c.done);
    root.setProperty("--w-divider", c.divider);
    root.setProperty("--w-primary", c.primary);
    root.setProperty("--w-on-primary", c.onPrimary);
    root.setProperty("--w-overdue", c.overdue);
    root.setProperty("--w-today", c.today);
    root.setProperty("--w-surface", c.surface);
    root.setProperty("--w-primary-container", c.primaryContainer);
    root.setProperty("--w-on-primary-container", c.onPrimaryContainer);
    root.setProperty("--w-error-container", c.errorContainer);
  };
  paint();
  onLook(paint);
  onThemeChange(paint);
}
