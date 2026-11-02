// Light, dark, or following the system. Switching cross-fades the whole page instead of letting
// every element run its own transition.

import { useSyncExternalStore } from "react";

export type Theme = "system" | "light" | "dark";

const KEY = "nexttodo.theme";
const dark = matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();

export const theme = (): Theme => (localStorage.getItem(KEY) as Theme | null) ?? "system";

export function applyTheme() {
  const isDark = theme() === "dark" || (theme() === "system" && dark.matches);
  document.documentElement.dataset.theme = isDark ? "dark" : "light";
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", isDark ? "#0F1512" : "#F5FBF6");
}

const crossFade = (change: () => void) => (document.startViewTransition ? document.startViewTransition(change) : change());

export function setTheme(t: Theme) {
  crossFade(() => {
    localStorage.setItem(KEY, t);
    applyTheme();
    for (const l of listeners) l();
  });
}

dark.addEventListener("change", () => crossFade(applyTheme));

export const useTheme = () =>
  useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    theme,
  );
