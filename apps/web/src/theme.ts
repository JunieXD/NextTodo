// Light, dark, or following the system; and the accent colour (docs/ui.md#颜色). Switching
// cross-fades the whole page instead of letting every element run its own transition.

import { SystemBars, SystemBarsStyle } from "@capacitor/core";
import { Hct, TonalPalette, argbFromHex, blueFromArgb, greenFromArgb, redFromArgb } from "@material/material-color-utilities";
import { useSyncExternalStore } from "react";
import { app } from "./api/http";
import { type Hex, parseHex } from "./domain/color";
import { NextTodo, inCard } from "./mobile";

export type Theme = "system" | "light" | "dark";

/** The accents besides the default, emerald, by their HCT hue. */
export const PRESETS = { lime: 120, cyan: 210, blue: 260, indigo: 285, purple: 310, pink: 350 } as const;
export type Preset = keyof typeof PRESETS;
/** The default, emerald, is the colours `:root` has; the others are made from a hue: a preset's, or
 * that of a colour of the user's own. */
export type Accent = "emerald" | Preset | Hex;

const KEY = "nexttodo.theme";
const ACCENT = "nexttodo.accent";
const dark = matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();

export const theme = (): Theme => (localStorage.getItem(KEY) as Theme | null) ?? "system";
export const isPreset = (a: string): a is Preset => Object.hasOwn(PRESETS, a);
/** As kept: anything else, written by a version to come say, is the default. */
export function accent(): Accent {
  const a = localStorage.getItem(ACCENT) ?? "emerald";
  return isPreset(a) ? a : (parseHex(a) ?? "emerald");
}
const isDark = () => theme() === "dark" || (theme() === "system" && dark.matches);

export const hueOf = (a: Preset | Hex) => (isPreset(a) ? PRESETS[a] : Hct.fromInt(argbFromHex(a)).hue);

const hex = (argb: number): Hex => `#${(argb & 0xffffff).toString(16).padStart(6, "0")}`;
const rgba = (argb: number, alpha: number) => `rgba(${redFromArgb(argb)}, ${greenFromArgb(argb)}, ${blueFromArgb(argb)}, ${alpha})`;

/** The chromas of the primary palette, the secondary, the neutral and the neutral variant. */
const CHROMAS = [36, 16, 6, 8];
const primaryTone = (dark: boolean) => (dark ? 80 : 40);

/** The colours of the accent of `hue`, as Material 3's Tonal Spot makes them, at the tones emerald's
 * are; what all accents share, such as white groups in the light theme, is left to `:root`. */
export function colors(hue: number, dark: boolean): Record<string, string> {
  const [p, s, n, v] = CHROMAS.map((chroma) => TonalPalette.fromHueAndChroma(hue, chroma));
  const at = (palette: TonalPalette, tone: number) => hex(palette.tone(tone));
  const on = n.tone(dark ? 90 : 10);
  if (dark)
    return {
      "--primary": at(p, primaryTone(true)),
      "--on-primary": at(p, 20),
      "--primary-container": at(p, 30),
      "--on-primary-container": at(p, 90),
      "--secondary-container": at(s, 30),
      "--on-secondary-container": at(s, 90),
      "--surface": at(n, 6),
      "--group-bg": at(n, 12),
      "--surface-low": at(n, 10),
      "--surface-container": at(n, 15.5),
      "--surface-high": at(n, 19),
      "--surface-highest": at(n, 24),
      "--seg-on": at(n, 26.5),
      "--on-surface": hex(on),
      "--on-surface-variant": at(v, 80),
      "--outline": at(v, 60),
      "--outline-variant": at(v, 30),
      "--rail": at(v, 40),
      "--inverse-surface": at(n, 90),
      "--inverse-on-surface": at(n, 20),
      "--inverse-primary": at(p, 40),
      "--bar-bg": rgba(n.tone(6), 0.78),
      "--press": rgba(on, 0.08),
    };
  return {
    "--primary": at(p, primaryTone(false)),
    "--primary-container": at(p, 90),
    "--on-primary-container": at(p, 10),
    "--secondary-container": at(s, 90),
    "--on-secondary-container": at(s, 10),
    "--surface": at(n, 98),
    "--surface-low": at(n, 96),
    "--surface-container": at(n, 94),
    "--surface-high": at(n, 92),
    "--surface-highest": at(n, 90),
    "--on-surface": hex(on),
    "--on-surface-variant": at(v, 30),
    "--outline": at(v, 50),
    "--outline-variant": at(v, 80),
    "--rail": at(v, 80),
    "--inverse-surface": at(n, 20),
    "--inverse-on-surface": at(n, 95),
    "--inverse-primary": at(p, 80),
    "--bar-bg": rgba(n.tone(98), 0.8),
    "--press": rgba(on, 0.06),
    "--shadow-1": `0 1px 2px ${rgba(on, 0.08)}, 0 1px 3px ${rgba(on, 0.06)}`,
    "--shadow-3": `0 4px 8px ${rgba(on, 0.1)}, 0 2px 3px ${rgba(on, 0.08)}`,
  };
}

/** `a`'s primary colour, as its swatch shows it. */
export const primaryOf = (a: Preset | Hex, dark: boolean) => hex(TonalPalette.fromHueAndChroma(hueOf(a), CHROMAS[0]).tone(primaryTone(dark)));

/** Where a colour of the user's own starts, to change a little: the primary the page shows now. */
export function startOwn(): Hex {
  const primary = getComputedStyle(document.documentElement).getPropertyValue("--primary");
  const hex = parseHex(primary);
  if (!hex) throw new Error(`--primary is not a colour: ${primary}`);
  return hex;
}

/** The colours put on the page for the accent, to take off for another. */
let applied: string[] = [];

export function applyTheme() {
  const root = document.documentElement;
  const a = accent();
  const made = a === "emerald" ? {} : colors(hueOf(a), isDark());
  root.dataset.theme = isDark() ? "dark" : "light";
  for (const name of applied) root.style.removeProperty(name);
  for (const [name, value] of Object.entries(made)) root.style.setProperty(name, value);
  applied = Object.keys(made);
  // Older engines keep the space before a custom property's value.
  const surface = getComputedStyle(root).getPropertyValue("--surface").trim();
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", surface);
  // Left alone, the apps color the status bar's icons by the system's appearance, not the page's.
  if (app === "ios") void SystemBars.setStyle({ style: isDark() ? SystemBarsStyle.Dark : SystemBarsStyle.Light });
  // The quick add card leaves the bars to the app it is over.
  else if (app === "android" && !inCard) void NextTodo.setSystemBars({ color: surface, dark: isDark() });
}

const crossFade = (change: () => void) => (document.startViewTransition ? document.startViewTransition(change) : change());

const changed = () => {
  applyTheme();
  for (const l of listeners) l();
};

// Kept at once, in the order chosen; the cross-fade only shows it, as it is by then.
export function setTheme(t: Theme) {
  localStorage.setItem(KEY, t);
  crossFade(changed);
}
export function setAccent(a: Accent) {
  localStorage.setItem(ACCENT, a);
  crossFade(changed);
}
/** A colour of the user's own as it is being picked: shown at once, not cross-faded. */
export function tuneAccent(color: Hex) {
  localStorage.setItem(ACCENT, color);
  changed();
}

dark.addEventListener("change", () => crossFade(changed));
// Switched in another tab, or the desktop app's other window.
addEventListener("storage", (e) => {
  if (e.key === KEY || e.key === ACCENT) changed();
});

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

export const useTheme = () => useSyncExternalStore(subscribe, theme);
export const useAccent = () => useSyncExternalStore(subscribe, accent);
export const useDark = () => useSyncExternalStore(subscribe, isDark);
