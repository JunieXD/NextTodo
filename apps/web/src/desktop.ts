// What only the desktop app does: deadline reminders, shown by the app while it runs, quick add from
// the menu bar or tray icon and the global shortcut, and the command line it carries. See
// docs/desktop.md.

import { invoke } from "@tauri-apps/api/core";
import { listen as on } from "@tauri-apps/api/event";
import { navigate } from "wouter/use-browser-location";
import { reminders } from "./domain/reminders";
import type { Store } from "./sync/store";

/** Changes coming close together are handled once, at most this long after the first: a stream of
 * them does not hold back a reminder. */
const SETTLE_MS = 2000;

/** Hands the reminders to come to the app (`apps/desktop/src-tauri/src/reminders.rs`). */
const hand = (list: { title: string; body: string; at: number; task: string }[]) => invoke("set_reminders", { reminders: list });

/** Keeps the app's reminders in step with the local copy, also when the window comes back: the time
 * has moved on. A handover that fails is reported to `failed`; the next change tries again. Returns
 * how to stop and clear them. */
export function watch(store: Store, failed: (e: unknown) => void): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  // A handover carries the reminders due since the last one too, which the app may not have shown
  // yet: of those, it shows the ones still wanted, and only those.
  let since = new Date();
  const update = () => {
    const now = new Date();
    // From `now` instead, should the clock have gone back since.
    const list = reminders(store.getView().board, since < now ? since : now);
    since = now;
    return hand(list.map((r) => ({ title: r.title, body: r.body, at: r.at.getTime(), task: r.task })));
  };
  const changed = () => {
    timer ??= setTimeout(() => {
      timer = undefined;
      update().catch(failed);
    }, SETTLE_MS);
  };
  const shown = () => document.visibilityState === "visible" && changed();
  const unsubscribe = store.subscribe(changed);
  document.addEventListener("visibilitychange", shown);
  update().catch(failed);
  return async () => {
    unsubscribe();
    document.removeEventListener("visibilitychange", shown);
    clearTimeout(timer);
    // Those due but not shown yet go too.
    await invoke("clear_reminders");
  };
}

/** Listens for quick add from the menu bar or tray icon and the global shortcut, and for clicks on
 * reminders. */
export async function listen() {
  await on("quick-add", () => navigate("/?add"));
  await on<string>("open-task", (e) => navigate(`/?open=${e.payload}`));
}

/** The command line the app carries: whether it is where a terminal finds it, and putting it there. */
export const cli = {
  installed: () => invoke<boolean>("cli_installed"),
  install: () => invoke<void>("install_cli"),
};
