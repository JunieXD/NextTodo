// What only the desktop app does: deadline reminders, shown by the app while it runs, the panel of the
// menu bar or tray icon and its shortcut, and the command line it carries. See docs/desktop.md.

import { invoke } from "@tauri-apps/api/core";
import { listen as on } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { navigate } from "wouter/use-browser-location";
import { app } from "./api/http";
import type { Attachment } from "./api/types/Attachment";
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
    const after = since < now ? since : now;
    since = now;
    const list = reminders(store.getView().board).filter((r) => r.at > after);
    return hand(list.map((r) => ({ title: r.title, body: r.body, at: r.at.getTime(), task: r.task.id })));
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

/** In the menu bar or tray panel rather than the main window (docs/desktop.md#菜单栏小窗). */
export const inPanel = app === "desktop" && getCurrentWindow().label === "panel";

/** Shows `path` in the main window: the panel's task or page, which it closes for. */
export const openMain = (path: string) => invoke<void>("open_main", { path });

export const hidePanel = () => getCurrentWindow().hide();

/** Hands attachment `a`'s file to the app (`src-tauri/src/attachments.rs`): the body is the file, not
 * JSON. */
const handOver = async <T>(command: string, a: Attachment, file: Blob) =>
  invoke<T>(command, new Uint8Array(await file.arrayBuffer()), { headers: { "x-id": a.id, "x-name": encodeURIComponent(a.name) } });

/** Opens the file with the system's default app. */
export const openAttachment = (a: Attachment, file: Blob) => handOver<void>("open_attachment", a, file);

/** Asks where to save the file; where it went, nothing when the user cancelled. */
export const saveAttachment = (a: Attachment, file: Blob) => handOver<string | null>("save_attachment", a, file);

/** Listens in the panel for being opened, to quick add; in the main window for what to show, from
 * the panel or a click on a reminder. */
export async function listen() {
  if (inPanel) await on("shown", () => navigate(`${location.pathname}?add`, { replace: true }));
  else await on<string>("open", (e) => navigate(e.payload));
}

/** Where the command line the app carries stands: on macOS its place may hold another one, `by` says
 * what (`apps/desktop/src-tauri/src/cli.rs`). */
export type CliStatus = { state: "installed" | "missing" } | { state: "taken"; by: string };

/** The command line the app carries: where it stands, and putting it where a terminal finds it, in
 * place of another one only when told to `replace` it. */
export const cli = {
  status: () => invoke<CliStatus>("cli_status"),
  install: (replace: boolean) => invoke<void>("install_cli", { replace }),
};

/** The panel's global shortcut, as `control+alt+KeyK`, and whether another app had it when the app
 * registered it (`apps/desktop/src-tauri/src/shortcut.rs`). */
export type Shortcut = { keys: string; taken: boolean };

/** The panel's global shortcut, if any; stopped while the settings take a new one. */
export const shortcut = {
  get: () => invoke<Shortcut | null>("shortcut"),
  set: (keys: string | null) => invoke<void>("set_shortcut", { keys }),
  pause: (paused: boolean) => invoke<void>("pause_shortcut", { paused }),
};
