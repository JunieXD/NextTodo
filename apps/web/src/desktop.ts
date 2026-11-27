// What only the desktop app does: reminders, shown by the app while it runs, the panel of the
// menu bar or tray icon and its shortcut, and the command line it carries. See docs/desktop.md.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { navigate } from "wouter/use-browser-location";
import { app } from "./api/http";
import type { Attachment } from "./api/types/Attachment";
import { digestText, digests, reminders } from "./domain/reminders";
import { isLocked, lockNow, onLock, privacyLock } from "./lock";
import { dailyDigest, hideReminders, onRemindSettings } from "./remind-settings";
import type { Store } from "./sync/store";

/** Changes coming close together are handled once, at most this long after the first: a stream of
 * them does not hold back a reminder. */
const SETTLE_MS = 2000;

/** Hands the reminders to come to the app (`apps/desktop/src-tauri/src/reminders.rs`): the daily
 * digests too, of no task. */
const hand = (list: { title: string; body: string; at: number; task?: string }[]) => invoke("set_reminders", { reminders: list });

/** Keeps the app's reminders in step with the local copy, also when the window comes back: the time
 * has moved on. A handover that fails is reported to `failed`; the next change tries again. Returns
 * how to stop and clear them. */
export function watch(store: Store, failed: (e: unknown) => void, widgetRemoved: () => void): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  // A handover carries the reminders due since the last one too, which the app may not have shown
  // yet: of those, it shows the ones still wanted, and only those.
  let since = new Date();
  const update = () => {
    const now = new Date();
    // From `now` instead, should the clock have gone back since.
    const after = since < now ? since : now;
    since = now;
    const { board } = store.getView();
    // Locked, they say no more than that something is due (docs/design.md#隐私锁).
    const hide = hideReminders() || isLocked();
    const { on, time } = dailyDigest();
    const list = [
      ...reminders(board, hide).map((r) => ({ title: r.title, body: r.body, at: r.at, task: r.task.id })),
      ...(on ? digests(board, time).map((d) => ({ at: d.at, ...digestText(d.tasks.map((t) => t.title), hide) })) : []),
    ];
    return hand(list.filter((r) => r.at > after).map((r) => ({ ...r, at: r.at.getTime() })));
  };
  const changed = () => {
    timer ??= setTimeout(() => {
      timer = undefined;
      update().catch(failed);
    }, SETTLE_MS);
  };
  const shown = () => document.visibilityState === "visible" && changed();
  const unsubscribe = store.subscribe(changed);
  const unset = onRemindSettings(changed);
  // At once: a reminder due in the meantime would say what it is of.
  const unwatch = onLock(() => void update().catch(failed));
  document.addEventListener("visibilitychange", shown);
  update().catch(failed);
  const widgets = desktopWidget.onChange((removed) => {
    if (removed) {
      localStorage.removeItem(widgetPageKey(removed));
      widgetRemoved();
    }
  });
  // Report now even if the session lasts a long time; its cleanup still waits for registration.
  const unlisten = widgets.catch(failed);
  return async () => {
    unsubscribe();
    unset();
    unwatch();
    document.removeEventListener("visibilitychange", shown);
    clearTimeout(timer);
    (await unlisten)?.();
    // Those due but not shown yet go too.
    await invoke("clear_reminders");
  };
}

const label = app === "desktop" ? getCurrentWindow().label : "main";
export const widgetLabel = label.startsWith("widget-") ? label : null;
export const inWidget = widgetLabel !== null;
/** In the menu bar or tray panel (docs/desktop.md#菜单栏小窗), or the desktop widget, which shows the
 * same, rather than the main window. */
export const inPanel = label === "panel" || inWidget;

/** Shows `path` in the main window: the panel's task or page, which it closes for. */
export const openMain = (path: string) => invoke<void>("open_main", { path });

/** Closes the panel, giving the focus back to the app it was opened from (`src-tauri/src/panel.rs`). */
export const hidePanel = () => invoke<void>("hide_panel");

/** Hands attachment `a`'s file to the app (`src-tauri/src/attachments.rs`): the body is the file, not
 * JSON. */
const handOver = async <T>(command: string, a: Pick<Attachment, "id" | "name">, file: Blob) =>
  invoke<T>(command, new Uint8Array(await file.arrayBuffer()), { headers: { "x-id": a.id, "x-name": encodeURIComponent(a.name) } });

/** Opens the file with the system's default app. */
export const openAttachment = (a: Attachment, file: Blob) => handOver<void>("open_attachment", a, file);

/** Asks where to save the file; where it went, nothing when the user cancelled. */
export const saveAttachment = (a: Pick<Attachment, "id" | "name">, file: Blob) => handOver<string | null>("save_attachment", a, file);

/** Listens, each window only to what is sent to it: the panel for being opened, to quick add; the main
 * window for what to show, from the panel or a click on a reminder, and for the menu's 锁定, which
 * with no lock set opens its settings. The widget is sent nothing. */
export async function listen() {
  if (inWidget) return;
  const here = getCurrentWindow();
  if (inPanel) return void (await here.listen("shown", () => navigate(`${location.pathname}?add`, { replace: true })));
  await here.listen<string>("open", (e) => navigate(e.payload));
  await here.listen("lock", () => (privacyLock() ? lockNow() : void openMain("/settings/lock")));
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

declare global {
  interface Window {
    /** The page a desktop widget just added was chosen to show, set by the app as it opens it. */
    nexttodoFirstPage?: string;
  }
}

export const widgetPageKey = (label: string) => `nexttodo.widgetPage.${label}`;

type Widgets = { labels: string[]; full: boolean };

export const desktopWidget = {
  list: () => invoke<Widgets>("widgets"),
  /** A new widget showing `page`, a path of the panel's. */
  add: (page: string) => invoke<void>("add_widget", { page }),
  remove: (label: string) => invoke<void>("remove_widget", { label }),
  onChange: (changed: (removed: string | null) => void) => getCurrentWindow().listen<string | null>("widgets-changed", ({ payload }) => changed(payload)),
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
