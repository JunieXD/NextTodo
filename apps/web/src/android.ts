// What only the Android app does: deadline reminders scheduled on the device, the list the
// home-screen widget shows, and the tasks written into the system calendar. See docs/android.md.

import { App } from "@capacitor/app";
import { type PermissionState, registerPlugin } from "@capacitor/core";
import { LocalNotifications, type PendingLocalNotificationSchema } from "@capacitor/local-notifications";
import { navigate } from "wouter/use-browser-location";
import type { Board } from "./domain/board";
import { type Reminder, reminder, reminders } from "./domain/reminders";
import type { Started } from "./local";
import type { Store } from "./sync/store";
import { back } from "./ui/back";

export type WidgetTextSize = "small" | "standard" | "large";

/** A calendar on the device the app may write to; `synced` unless it is on this device alone. */
export interface DeviceCalendar {
  id: number;
  name: string;
  account: string;
  synced: boolean;
}

/** Where the tasks with a deadline are written: the app's own calendar, or one of the device's by id. */
export type CalendarTarget = "own" | number;

/** The app's own plugin, in `apps/android`. */
interface NextTodoPlugin {
  /** Hands the widget its list, as JSON in the shape of the `next` tool's `tasks`, as of the change
   * `seq`; without one, as when signing out, it replaces whatever the widget has. */
  updateWidget(o: { tasks: string; seq?: number }): Promise<void>;
  /** How large the widget's text is, which it keeps. */
  widgetTextSize(): Promise<{ size: WidgetTextSize }>;
  setWidgetTextSize(o: { size: WidgetTextSize }): Promise<void>;
  /** Where the tasks with a deadline are written in the system calendar, if anywhere
   * (docs/android.md#写进系统日历). */
  calendar(): Promise<{ target?: CalendarTarget }>;
  /** The device's calendars the app may write to, but its own; with the permission. */
  calendars(): Promise<{ calendars: DeviceCalendar[] }>;
  /** Writes there from now on, or nowhere without a target; what was written before goes. */
  setCalendar(o: { target?: CalendarTarget }): Promise<void>;
  /** Makes the calendar show `events`, as JSON in the shape `calendarEvents` gives. */
  updateCalendar(o: { events: string }): Promise<void>;
  /** Deletes what was written, keeping where to write. */
  clearCalendar(): Promise<void>;
  checkPermissions(): Promise<{ calendar: PermissionState }>;
  requestPermissions(): Promise<{ calendar: PermissionState }>;
  /** The system settings page of this app, where autostart and battery use are allowed. */
  openAppSettings(): Promise<void>;
  /** The APK's version, and its native version, which builds of the web app may need. */
  appInfo(): Promise<{ version: string; native: number }>;
  /** Says the web app has started, so that the build being tried is kept. */
  webReady(): Promise<void>;
  /** Downloads `server`'s build of the web app for the next start; the version when there was one. */
  updateWeb(o: { server: string }): Promise<{ staged?: string }>;
  /** Downloads the APK and hands it to the system installer; `started: false` when the app may not
   * install apps yet, and the settings page that allows it opened instead. */
  installApp(o: { url: string }): Promise<{ started: boolean }>;
  /** Local mode (docs/local.md), as the app's start opened it; should the server not have started,
   * tries again, and rejects with code `locked` while an earlier process of the app has the data. */
  openLocal(): Promise<Started>;
  /** Starts local mode's server, to enter it: whose data it has. */
  startLocal(): Promise<{ user: string }>;
  /** Enters local mode or leaves it, from the next start. */
  setLocalMode(o: { on: boolean }): Promise<void>;
  /** Ends the app's process and opens the app again. */
  restart(): Promise<void>;
  /** Asks where to save a backup, offering `name`, and writes it there; `saved` is what the file is
   * called, nothing when the user cancelled. */
  backupLocal(o: { name: string }): Promise<{ saved?: string }>;
  /** Asks for a backup and prepares it to replace the data; whether the user picked one. */
  prepareRestore(): Promise<{ picked: boolean }>;
  /** The backup prepared replaces the data at the next start. */
  commitRestore(): Promise<void>;
}

export const NextTodo = registerPlugin<NextTodoPlugin>("NextTodo");

/** How many reminders are kept scheduled ahead: the system limits an app's alarms. */
const AHEAD = 50;

/** Changes coming close together are handled once, at most this long after the first: a stream of
 * them does not hold back a reminder. */
const SETTLE_MS = 2000;

/** Updates and clearing go one at a time, across sessions too: an update may wait for the user to
 * answer the permission prompt, and a session's clearing up must not come after the next one's
 * first update. */
let queue = Promise.resolve();
function inTurn(f: () => Promise<void>) {
  const done = queue.then(f);
  queue = done.catch(() => {});
  return done;
}

/** What the widget lists: the ready tasks, as the `next` tool gives them. */
export const widgetTasks = (board: Board) =>
  board.next().map((t) => ({ id: t.id, number: t.number, title: t.title, folder: board.folderPath(t.folder_id), due: t.due, completions: t.completions }));

/** What the system calendar shows when it is written to: the open tasks with a deadline, and where
 * each is. */
export const calendarEvents = (board: Board) =>
  board.snapshot.tasks.flatMap((t) =>
    t.done_at === null && t.due
      ? [{ task: t.id, title: t.title, due: t.due, description: t.folder_id ? `${board.place(t.folder_id)}\n来自 NextTodo` : "来自 NextTodo" }]
      : [],
  );

/** Writes the calendar for the session being watched: none once it has stopped, so that nothing
 * asked of a session that has ended, such as while the permission prompt waited, writes its tasks. */
let writeCalendar: (() => Promise<void>) | undefined;

/** Writes the tasks with a deadline to `target` from now on, or nowhere without one. */
export const setCalendar = (target: CalendarTarget | undefined) =>
  inTurn(async () => {
    await NextTodo.setCalendar({ target });
    await writeCalendar?.();
  });

/** Keeps the reminders, the widget and the calendar in step with the local copy, also when the app
 * comes back to the front: the time has moved on, and the permissions may have changed. An update
 * that fails is reported to `failed`; the next change tries again. Returns how to stop and clear them. */
export function watch(store: Store, failed: (e: unknown) => void): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const update = async () => {
    const { board } = store.getView();
    // With the change the copy is as of: the widget keeps a list it fetched since, should the copy
    // be behind the server.
    await NextTodo.updateWidget({ tasks: JSON.stringify(widgetTasks(board)), seq: store.since });
    const now = new Date();
    const wanted = reminders(board, now).slice(0, AHEAD);
    // Without the permission for exact alarms, the plugin would open the system settings on every
    // schedule; ask for inexact ones instead. The settings page offers to allow it.
    const exact = (await LocalNotifications.checkExactNotificationSetting()).exact_alarm === "granted";
    const { notifications } = await LocalNotifications.getPending();
    // The plugin gives the time back in a format of its own, so each reminder carries its time.
    const same = (w: Reminder, n: PendingLocalNotificationSchema) =>
      w.id === n.id && w.title === n.title && w.body === n.body && n.extra?.at === w.at.getTime() && n.extra?.exact === exact;
    // An inexact alarm can come a little after its time: one still on its way stays while its task
    // still wants it.
    const coming = (n: PendingLocalNotificationSchema) => {
      const t = n.extra && n.extra.at <= now.getTime() && board.task(n.extra.task);
      const r = t && reminder(board, t);
      return !!r && r.at.getTime() === n.extra.at && r.title === n.title && r.body === n.body;
    };
    const stale = notifications.filter((n) => !coming(n) && !wanted.some((w) => same(w, n)));
    if (stale.length) await LocalNotifications.cancel({ notifications: stale.map((n) => ({ id: n.id })) });
    const fresh = wanted.filter((w) => !notifications.some((n) => same(w, n)));
    if (!fresh.length) return;
    if ((await LocalNotifications.checkPermissions()).display !== "granted" && (await LocalNotifications.requestPermissions()).display !== "granted") return;
    await LocalNotifications.schedule({
      notifications: fresh.map((r) => ({
        id: r.id,
        title: r.title,
        body: r.body,
        schedule: { at: r.at, allowWhileIdle: true },
        isExactNotification: exact,
        extra: { task: r.task, at: r.at.getTime(), exact },
      })),
    });
  };
  const calendar = () => NextTodo.updateCalendar({ events: JSON.stringify(calendarEvents(store.getView().board)) });
  // The calendar in a turn of its own: failing, it holds back neither the reminders nor the widget.
  const run = () => {
    inTurn(update).catch(failed);
    inTurn(calendar).catch(failed);
  };
  const changed = () => {
    timer ??= setTimeout(() => {
      timer = undefined;
      run();
    }, SETTLE_MS);
  };
  const shown = () => document.visibilityState === "visible" && changed();
  const unsubscribe = store.subscribe(changed);
  document.addEventListener("visibilitychange", shown);
  writeCalendar = calendar;
  run();
  return async () => {
    // The next session may have started watching already.
    if (writeCalendar === calendar) writeCalendar = undefined;
    unsubscribe();
    document.removeEventListener("visibilitychange", shown);
    clearTimeout(timer);
    await clear(failed);
  };
}

/** Empties the widget, takes away every reminder, those shown already too, and the events written in
 * the calendar; after the updates under way. The calendar failing goes to `failed` and holds back
 * neither the rest nor the sign-out: what is left there stays noted, and the next session's first
 * update deletes it, reporting should it fail again. Before a session there is no one to tell, and
 * that update alone reports. */
export const clear = async (failed: (e: unknown) => void = () => {}) => {
  const calendar = inTurn(() => NextTodo.clearCalendar()).catch(failed);
  await inTurn(async () => {
    await NextTodo.updateWidget({ tasks: "[]" });
    const { notifications } = await LocalNotifications.getPending();
    if (notifications.length) await LocalNotifications.cancel({ notifications: notifications.map((n) => ({ id: n.id })) });
    await LocalNotifications.removeAllDeliveredNotifications();
  });
  await calendar;
};

/** Opens what a notification or the widget points at: `nexttodo://task/<id>`, `nexttodo://add` or
 * `nexttodo://home`. */
function open(url: string) {
  const [, kind, id] = /^nexttodo:\/\/(task|add|home)\/?(.*)$/.exec(url) ?? [];
  if (kind === "task") navigate(`/?open=${id}`);
  else if (kind === "add") navigate("/?add");
  else if (kind === "home") navigate("/");
}

/** Listens for taps on notifications and the widget. The plugins keep the one that started the app
 * until there is a listener. */
export async function listen() {
  await LocalNotifications.addListener("localNotificationActionPerformed", (e) => open(`nexttodo://task/${e.notification.extra.task}`));
  await App.addListener("appUrlOpen", (e) => open(e.url));
  await App.addListener("backButton", () => {
    if (!back()) void App.minimizeApp();
  });
}
