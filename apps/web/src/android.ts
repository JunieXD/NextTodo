// What only the Android app does: deadline reminders shown by the device, the list the home-screen
// widget shows, the changes the widget and reminders made, and the tasks written into the system
// calendar. See docs/android.md.

import { App } from "@capacitor/app";
import { type PermissionState, registerPlugin } from "@capacitor/core";
import { Preferences } from "@capacitor/preferences";
import { navigate } from "wouter/use-browser-location";
import { aiReady, loadAiMode } from "./ai";
import type { Mutation } from "./api/types/Mutation";
import type { Board } from "./domain/board";
import { reminders } from "./domain/reminders";
import type { Started } from "./local";
import type { Store } from "./sync/store";
import { back } from "./ui/back";
import { settingsUrl } from "./views/nav";

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

/** What reminders are allowed: `notify`, to show; `exact`, to come on time; and whether the reminder
 * card is on and allowed to draw over other apps (`overlay`). */
export interface ReminderSettings {
  notify: boolean;
  exact: boolean;
  card: boolean;
  overlay: boolean;
}

/** The app's own plugin, in `apps/android`. */
interface NextTodoPlugin {
  /** Hands the device the reminders, as JSON in the shape `deviceReminders` gives; the first time
   * there are some, asks for notifications. */
  setReminders(o: { reminders: string }): Promise<void>;
  /** Takes every reminder away, those put off and those shown too. */
  clearReminders(): Promise<void>;
  reminderSettings(): Promise<ReminderSettings>;
  /** Asks for notifications where Android asks; whether they are allowed. */
  allowNotifications(): Promise<{ notify: boolean }>;
  openExactAlarmSettings(): Promise<void>;
  /** Turns the reminder card on or off (docs/android.md#弹窗提醒). */
  setCard(o: { on: boolean }): Promise<void>;
  /** The system page that allows drawing over other apps, which the card needs. */
  openOverlaySettings(): Promise<void>;
  /** Signing in as `user`: the changes the widget, reminders and the add popup made for anyone else
   * go, before the token is written, so that none is sent under it. */
  signIn(o: { user: string }): Promise<void>;
  /** The changes the widget, reminders and the add popup made for `user` and could not send yet, as
   * JSON `[{id, ops}]`, for the app to take in as its own; they go from there too, the server taking
   * each once (docs/android.md#原生发出的改动). */
  takeChanges(o: { user: string }): Promise<{ changes: string }>;
  /** Deletes those changes, and the add popup's draft: signed out. */
  forgetChanges(): Promise<void>;
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
  requestPermissions(o: { permissions: "calendar"[] }): Promise<{ calendar: PermissionState }>;
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
  board.next().map((t) => ({
    id: t.id,
    number: t.number,
    title: t.title,
    folder: board.folderPath(t.folder_id),
    due: t.due,
    completions: t.completions,
    step: board.step(t),
  }));

/** What the device reminds of (docs/android.md#截止提醒): every reminder, past ones too, with whether
 * the task waits for prerequisites, and for its start date, which the device reads by its own day. */
export const deviceReminders = (board: Board) =>
  reminders(board).map((r) => ({
    task: r.task.id,
    number: r.number,
    at: r.at.getTime(),
    title: r.title,
    body: r.body,
    waiting: board.waitingOn(r.task),
    start: r.task.start,
  }));

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

/** Keeps the reminders, the widget and the calendar in step with the local copy of `user`'s data, also
 * when the app comes back to the front: the time has moved on, and the permissions may have changed.
 * Takes in the changes made by the widget, reminders and the add popup meanwhile, and the mode the
 * popup last used; tells the popup whether AI organizing is set up. An update that fails is reported
 * to `failed`; the next change tries again. Returns how to stop and clear them. */
export function watch(store: Store, user: string, failed: (e: unknown) => void): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const update = async () => {
    const { board } = store.getView();
    // With the change the copy is as of: the widget keeps a list it fetched since, should the copy
    // be behind the server.
    await NextTodo.updateWidget({ tasks: JSON.stringify(widgetTasks(board)), seq: store.since });
    await NextTodo.setReminders({ reminders: JSON.stringify(deviceReminders(board)) });
  };
  const calendar = () => NextTodo.updateCalendar({ events: JSON.stringify(calendarEvents(store.getView().board)) });
  const takeOver = async () => {
    const { changes } = await NextTodo.takeChanges({ user });
    store.adopt(JSON.parse(changes) as Mutation[]);
  };
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
  const shown = () => {
    if (document.visibilityState !== "visible") return;
    takeOver().catch(failed);
    loadAiMode().catch(failed);
    changed();
  };
  const unsubscribe = store.subscribe(changed);
  document.addEventListener("visibilitychange", shown);
  writeCalendar = calendar;
  takeOver().catch(failed);
  // Unless said since: by the settings page, or by the session ending.
  const asked = ++aiReadySaid;
  aiReady(user)
    .then((ready) => (asked === aiReadySaid ? setAiReady(ready) : undefined))
    .catch(failed);
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

/** How many times the add popup has been told whether AI organizing is set up, or asked about it: an
 * answer to an earlier question must not say otherwise than what came since. */
let aiReadySaid = 0;

/** Tells the add popup whether AI organizing is set up (docs/android.md#添加弹窗). */
export const setAiReady = (ready: boolean) => {
  aiReadySaid++;
  return Preferences.set({ key: "aiReady", value: String(ready) });
};

/** Empties the widget, takes away every reminder, those shown already too, and the events written in
 * the calendar; after the updates under way. The calendar failing goes to `failed` and holds back
 * neither the rest nor the sign-out: what is left there stays noted, and the next session's first
 * update deletes it, reporting should it fail again. Before a session there is no one to tell, and
 * that update alone reports. */
export const clear = async (failed: (e: unknown) => void = () => {}) => {
  aiReadySaid++;
  const calendar = inTurn(() => NextTodo.clearCalendar()).catch(failed);
  await inTurn(async () => {
    await NextTodo.updateWidget({ tasks: "[]" });
    await NextTodo.clearReminders();
    await Preferences.remove({ key: "aiReady" });
  });
  await calendar;
};

/** Signed out: the changes the widget, reminders and the add popup could not send yet go, with the
 * popup's draft. A session that just ended keeps them for the same user's next sign-in. */
export const forget = () => NextTodo.forgetChanges();

/** Opens what a notification, the widget or the add popup points at: `nexttodo://task/<id>`,
 * `nexttodo://home`, or `nexttodo://ai`, the settings of AI organizing. */
function open(url: string) {
  const [, kind, id] = /^nexttodo:\/\/(task|home|ai)\/?(.*)$/.exec(url) ?? [];
  if (kind === "task") navigate(`/?open=${id}`);
  else if (kind === "home") navigate("/");
  else if (kind === "ai") navigate(settingsUrl("ai"));
}

/** Listens for taps on notifications, the widget and the add popup. The plugin keeps the one that
 * started the app until there is a listener. */
export async function listen() {
  await App.addListener("appUrlOpen", (e) => open(e.url));
  await App.addListener("backButton", () => {
    if (!back()) void App.minimizeApp();
  });
}
