// How large the desktop app's windows draw, each its own: the main window, the panel and the desktop
// widget, zoomed as a browser zooms a page (docs/desktop.md#界面大小); the widget's is its text size
// (docs/desktop.md#桌面小组件).

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useSyncExternalStore } from "react";
import { inPanel, inWidget } from "./desktop";

export type Zoom = "small" | "standard" | "large";
export type Zoomed = "main" | "panel" | "widget";

/** By default the panel and the widget draw smaller than the main window: at the phone's sizes, they
 * look large in their 360 × 480 and 340 × 440. */
const PANEL = { small: 0.8125, standard: 0.875, large: 1 };
const SCALES: Record<Zoomed, Record<Zoom, number>> = {
  main: { small: 0.875, standard: 1, large: 1.125 },
  panel: PANEL,
  widget: PANEL,
};

const key = (w: Zoomed) => `nexttodo.zoom.${w}`;
const here: Zoomed = inWidget ? "widget" : inPanel ? "panel" : "main";
const listeners = new Set<() => void>();

/** As chosen; one this version doesn't know, written by a later one, is the standard. */
export function zoom(w: Zoomed): Zoom {
  const z = localStorage.getItem(key(w));
  return z === "small" || z === "large" ? z : "standard";
}

async function zoomTo(scale: number) {
  await getCurrentWebview().setZoom(scale);
  // For what lines up with the macOS window's buttons, which don't zoom (index.css).
  document.documentElement.style.setProperty("--zoom", String(scale));
}

/** Zooms this window as chosen for it. */
export const applyZoom = () => zoomTo(SCALES[here][zoom(here)]);

/** Chooses for `w`, once this window has zoomed should it be `w`; another window takes it from the
 * storage event. */
export async function setZoom(w: Zoomed, z: Zoom) {
  if (w === here) await zoomTo(SCALES[w][z]);
  localStorage.setItem(key(w), z);
  for (const l of listeners) l();
}

addEventListener("storage", (e) => {
  if (e.key === key(here)) void applyZoom();
  if (e.key === key("main") || e.key === key("panel") || e.key === key("widget")) for (const l of listeners) l();
});

export const useZoom = (w: Zoomed) =>
  useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => zoom(w),
  );
