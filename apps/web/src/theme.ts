// Light, dark, or following the system; and the accent colour (docs/ui.md#颜色). Switching
// cross-fades the whole page instead of letting every element run its own transition.

import { SystemBars, SystemBarsStyle } from "@capacitor/core";
import { useSyncExternalStore } from "react";
import { app } from "./api/http";
import { NextTodo, inCard } from "./mobile";

export type Theme = "system" | "light" | "dark";
/** The default, emerald, is the colours `:root` has; the others, `:root[data-accent]`'s. */
export type Accent = "emerald" | "lime" | "cyan" | "blue" | "indigo" | "purple" | "pink";

const KEY = "nexttodo.theme";
const ACCENT = "nexttodo.accent";
const dark = matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();

export const theme = (): Theme => (localStorage.getItem(KEY) as Theme | null) ?? "system";
export const accent = (): Accent => (localStorage.getItem(ACCENT) as Accent | null) ?? "emerald";

export function applyTheme() {
  const isDark = theme() === "dark" || (theme() === "system" && dark.matches);
  const root = document.documentElement;
  root.dataset.theme = isDark ? "dark" : "light";
  root.dataset.accent = accent();
  // Older engines keep the space before a custom property's value.
  const surface = getComputedStyle(root).getPropertyValue("--surface").trim();
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", surface);
  // Left alone, the apps color the status bar's icons by the system's appearance, not the page's.
  if (app === "ios") void SystemBars.setStyle({ style: isDark ? SystemBarsStyle.Dark : SystemBarsStyle.Light });
  // The quick add card leaves the bars to the app it is over.
  else if (app === "android" && !inCard) void NextTodo.setSystemBars({ color: surface, dark: isDark });
}

const crossFade = (change: () => void) => (document.startViewTransition ? document.startViewTransition(change) : change());

const set = (key: string, value: string) =>
  crossFade(() => {
    localStorage.setItem(key, value);
    applyTheme();
    for (const l of listeners) l();
  });

export const setTheme = (t: Theme) => set(KEY, t);
export const setAccent = (a: Accent) => set(ACCENT, a);

dark.addEventListener("change", () => crossFade(applyTheme));
// Switched in another tab, or the desktop app's other window.
addEventListener("storage", (e) => {
  if (e.key !== KEY && e.key !== ACCENT) return;
  applyTheme();
  for (const l of listeners) l();
});

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

export const useTheme = () => useSyncExternalStore(subscribe, theme);
export const useAccent = () => useSyncExternalStore(subscribe, accent);
