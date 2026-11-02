// What only the macOS app does: deadline reminders, shown by the app while it runs, and quick add
// from the menu bar and the global shortcut. See docs/macos.md.

import { invoke } from "@tauri-apps/api/core";
import { listen as on } from "@tauri-apps/api/event";
import { navigate } from "wouter/use-browser-location";
import { reminders } from "./domain/reminders";
import type { Store } from "./sync/store";

/** Changes coming close together are handled once. */
const SETTLE_MS = 2000;

/** Hands the reminders to come to the app (`apps/desktop/src-tauri/src/reminders.rs`). */
const hand = (list: { title: string; body: string; at: number }[]) => invoke("set_reminders", { reminders: list });

/** Keeps the app's reminders in step with the local copy, also when the window comes back: the time
 * has moved on. A handover that fails is reported to `failed`; the next change tries again. Returns
 * how to stop and clear them. */
export function watch(store: Store, failed: (e: unknown) => void): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const update = () => hand(reminders(store.getView().board, new Date()).map((r) => ({ title: r.title, body: r.body, at: r.at.getTime() })));
  const changed = () => {
    clearTimeout(timer);
    timer = setTimeout(() => update().catch(failed), SETTLE_MS);
  };
  const shown = () => document.visibilityState === "visible" && changed();
  const unsubscribe = store.subscribe(changed);
  document.addEventListener("visibilitychange", shown);
  changed();
  return async () => {
    unsubscribe();
    document.removeEventListener("visibilitychange", shown);
    clearTimeout(timer);
    await hand([]);
  };
}

/** Listens for quick add from the menu bar icon and the global shortcut. */
export async function listen() {
  await on("quick-add", () => navigate("/?add"));
}
