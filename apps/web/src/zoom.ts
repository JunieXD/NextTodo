// How large the desktop app's two windows draw, each its own: the main window and the panel, zoomed
// as a browser zooms a page (docs/desktop.md#界面大小).

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useSyncExternalStore } from "react";
import { inPanel } from "./desktop";

export type Zoom = "small" | "standard" | "large";
export type Zoomed = "main" | "panel";

/** By default the panel draws smaller than the main window: at the phone's sizes, it looks large in
 * its 360 × 480. */
const SCALES: Record<Zoomed, Record<Zoom, number>> = {
  main: { small: 0.875, standard: 1, large: 1.125 },
  panel: { small: 0.8125, standard: 0.875, large: 1 },
};

const key = (w: Zoomed) => `nexttodo.zoom.${w}`;
const here: Zoomed = inPanel ? "panel" : "main";
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

/** Chooses for `w`, once this window has zoomed should it be `w`; the other window takes it from the
 * storage event. */
export async function setZoom(w: Zoomed, z: Zoom) {
  if (w === here) await zoomTo(SCALES[w][z]);
  localStorage.setItem(key(w), z);
  for (const l of listeners) l();
}

addEventListener("storage", (e) => {
  if (e.key === key(here)) void applyZoom();
  if (e.key === key("main") || e.key === key("panel")) for (const l of listeners) l();
});

export const useZoom = (w: Zoomed) =>
  useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => zoom(w),
  );
