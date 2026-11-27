// What only the phone apps do: reminders shown by the device, the changes the reminders made,
// and on Android the list the home-screen widget shows, the changes it made and the tasks written into
// the system calendar. See docs/android.md and docs/ios.md.

import { App } from "@capacitor/app";
import { Capacitor, type PermissionState, registerPlugin } from "@capacitor/core";
import { navigate } from "wouter/use-browser-location";
import { app } from "./api/http";
import type { Attachment } from "./api/types/Attachment";
import { sizeLimit, tooLarge } from "./attach";
import type { FolderId } from "./api/types/FolderId";
import type { Mutation } from "./api/types/Mutation";
import type { Task } from "./api/types/Task";
import { type Board, nowOf } from "./domain/board";
import { agenda } from "./domain/calendar";
import { day as parseDay, isoDate, monthGrid } from "./domain/dates";
import { dayLabel, holiday } from "./domain/holidays";
import { digests, reminders } from "./domain/reminders";
import { type Digest, dailyDigest, hideReminders, onRemindSettings } from "./remind-settings";
import type { Started } from "./local";
import type { Store } from "./sync/store";
import type { WidgetLook } from "./domain/widget-look";
import { back } from "./ui/back";
import { forUser } from "./ui/feedback";
import { nextFolders, onNextFolders, readyIn } from "./next-folders";
import { prefsOf } from "./prefs";

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

/** The app's own plugin, in `apps/android` and `apps/ios`; iOS has what it needs only
 * (docs/ios.md#结构). */
interface NextTodoPlugin {
  /** Hands the device the reminders and the daily digests, as JSON in the shapes `deviceReminders`
   * and `deviceDigests` give, of the local copy as of change `seq`: the device leaves out the times
   * done on it that the copy may not have yet (docs/android/reminders.md). The first time there are
   * some, asks for notifications. */
  setReminders(o: { reminders: string; digest: string; seq: number }): Promise<void>;
  /** Takes every reminder away, those put off and those shown too. */
  clearReminders(): Promise<void>;
  reminderSettings(): Promise<ReminderSettings>;
  /** Asks for notifications where Android asks; whether they are allowed. */
  allowNotifications(): Promise<{ notify: boolean }>;
  openExactAlarmSettings(): Promise<void>;
  /** Shows the system bars as over a page of `color`, `#rrggbb` (docs/android.md#结构). */
  setSystemBars(o: { color: string; dark: boolean }): Promise<void>;
  /** The system settings choosing the sound and vibration of reminders (docs/android/reminders.md#通知). */
  openReminderSound(): Promise<void>;
  /** Turns the reminder card on or off (docs/android/reminders.md#弹窗提醒). */
  setCard(o: { on: boolean }): Promise<void>;
  /** The system page that allows drawing over other apps, which the card needs. */
  openOverlaySettings(): Promise<void>;
  /** Signing in as `user`: the changes the widget and reminders made for anyone else go, before the
   * token is written, so that none is sent under it. */
  signIn(o: { user: string }): Promise<void>;
  /** The changes the widget and reminders made for `user` and could not send yet, as
   * JSON `[{id, ops}]`, for the app to take in as its own; they go from there too, the server taking
   * each once (docs/android.md#原生发出的改动). */
  takeChanges(o: { user: string }): Promise<{ changes: string }>;
  /** Deletes those changes: signed out. */
  forgetChanges(): Promise<void>;
  /** Hands the widget its list, as JSON in the shape of the `next` tool's `tasks`, as of the change
   * `seq`; without one, as when signing out, it replaces whatever the widget has. At `nextStart`, as
   * the `next` tool's `next_start` gives it, the widget fetches its list itself. */
  updateWidget(o: { tasks: string; seq?: number; nextStart?: string; folders?: string }): Promise<void>;
  /** Hands the agenda widget its data, as JSON in the shape of the `agenda` tool's, as of the change
   * `seq`; without one, as when signing out, it replaces whatever the widget has. */
  updateAgenda(o: { agenda: string; seq?: number }): Promise<void>;
  /** Hands the widgets what is written with each day (`widgetDays`), as JSON. */
  updateDays(o: { days: string }): Promise<void>;
  /** How the widgets look, which they keep. */
  widgetLook(): Promise<WidgetLook>;
  setWidgetLook(look: WidgetLook): Promise<void>;
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
  /** The system settings page of this app, where autostart and battery use are allowed; on iOS, its
   * notification settings. */
  openAppSettings(): Promise<void>;
  /** The installed version, and its native version, which builds of the web app may need. */
  appInfo(): Promise<{ version: string; native: number }>;
  /** Says the web app has started, so that the build being tried is kept. */
  webReady(): Promise<void>;
  /** Shows the keyboard for the field just focused, which a page cannot but on a tap. */
  showKeyboard(): Promise<void>;
  /** What another app shared, when it opened the quick add card: the text, and the files copied into
   * the app's cache, but for those larger than `sizeLimit` allows, or why one could not be read;
   * nothing else, or once taken. */
  takeShared(limit: ReturnType<typeof sizeLimit>): Promise<{
    text?: string;
    files?: ({ path: string; name: string; type: string } | { name: string; tooLarge: true } | { error: string })[];
  }>;
  /** Closes the quick add card, back to the app it was opened over. */
  closeCard(): Promise<void>;
  /** Opens the app at `url`, `nexttodo://…`, and closes the quick add card. */
  openApp(o: { url: string }): Promise<void>;
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
  /** Ends the app's process and opens the app again; iOS, where an app cannot, stops local mode's
   * server and loads the page again (docs/ios.md#本机模式). */
  restart(): Promise<void>;
  /** Asks where to save a backup, offering `name`, and writes it there; `saved` is what the file is
   * called, nothing when the user cancelled. */
  backupLocal(o: { name: string }): Promise<{ saved?: string }>;
  /** Asks for a backup and prepares it to replace the data; whether the user picked one. */
  prepareRestore(): Promise<{ picked: boolean }>;
  /** The backup prepared replaces the data at the next start. */
  commitRestore(): Promise<void>;
  /** Writes the `first` piece of attachment `id`'s file, or the next, base64 `data`, to be opened or
   * saved as `name` (docs/android.md#附件); the `last` puts it in place. */
  writeAttachment(o: { id: string; name: string; data: string; first: boolean; last: boolean }): Promise<void>;
  /** Hands the file written to an app for its `type`. */
  openAttachment(o: { id: string; name: string; type: string }): Promise<void>;
  /** Asks where to save the file written and copies it there; `saved` is what the file is called,
   * nothing when the user cancelled. */
  saveAttachment(o: { id: string; name: string; type: string }): Promise<{ saved?: string }>;
}

export const NextTodo = registerPlugin<NextTodoPlugin>("NextTodo");

/** This page is Android's quick add card over another app, not the app (docs/android.md#快速添加小窗). */
export const inCard = app === "android" && location.pathname === "/quick-add";

/** What another app shared to the quick add card, its files fetched from where the app copied them;
 * and why each of the others is not there. */
export async function takeShared(): Promise<{ text: string; files: File[]; errors: string[] }> {
  const { text = "", files = [] } = await NextTodo.takeShared(sizeLimit());
  const fetched: File[] = [];
  const errors: string[] = [];
  for (const f of files) {
    if ("error" in f) errors.push(f.error);
    else if ("tooLarge" in f) errors.push(tooLarge(f.name));
    else {
      try {
        const r = await window.CapacitorWebFetch(Capacitor.convertFileSrc(f.path));
        if (!r.ok) throw new Error(`${r.status}`);
        fetched.push(new File([await r.blob()], f.name, { type: f.type }));
      } catch (e) {
        errors.push(forUser(`读取“${f.name}”`, e));
      }
    }
  }
  return { text, files: fetched, errors };
}

/** How much of a file goes over the bridge at a time. */
const PIECE = 512 * 1024;

const base64 = (b: Blob) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve((reader.result as string).split(",", 2)[1]);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(b);
  });

/** Files handed to the app go one at a time: two of the same, opened twice say, would be written
 * together. */
let handing = Promise.resolve();

/** Hands attachment `a`'s file to the app, a piece at a time, to open or save. */
export function writeAttachment(a: Pick<Attachment, "id" | "name">, file: Blob): Promise<void> {
  const done = handing.then(async () => {
    for (let at = 0; at === 0 || at < file.size; at += PIECE) {
      const data = await base64(file.slice(at, at + PIECE));
      await NextTodo.writeAttachment({ id: a.id, name: a.name, data, first: at === 0, last: at + PIECE >= file.size });
    }
  });
  // eslint-disable-next-line no-restricted-syntax -- the caller hears of the failure from `done`; the next goes on
  handing = done.catch(() => {});
  return done;
}

/** Changes coming close together are handled once, at most this long after the first: a stream of
 * them does not hold back a reminder. */
const SETTLE_MS = 2000;

/** Updates and clearing go one at a time, across sessions too: an update may wait for the user to
 * answer the permission prompt, and a session's clearing up must not come after the next one's
 * first update. */
let queue = Promise.resolve();
function inTurn(f: () => Promise<void>) {
  const done = queue.then(f);
  // eslint-disable-next-line no-restricted-syntax -- the caller hears of the failure from `done`; the next goes on
  queue = done.catch(() => {});
  return done;
}

/** What the widget lists: the ready tasks in the folders “接下来” lists, as the `next` tool gives them. */
export const widgetTasks = (board: Board, folders: (FolderId | null)[]) =>
  readyIn(board, folders).map((t) => ({
    id: t.id,
    number: t.number,
    title: t.title,
    folder: board.folderPath(t.folder_id),
    due: t.due,
    completions: t.completions,
    step: board.step(t),
    ...dueBy(board, t),
  }));

/** The deadline of a task after `t` that it is to make, as the `next` tool gives it
 * (docs/design/dates.md#后续的截止). */
function dueBy(board: Board, t: Task) {
  const by = board.dueBy().get(t.id);
  return by && { due_by: { date: by.due.date, ...(by.due.time && { time: by.due.time }), number: by.number, title: by.title } };
}

/** How many days from `today` the widgets' data has: to the last of next month's six weeks, which the
 * “月历” widget shows (docs/android/widgets.md#月历). The widgets ask the server for as many (`Agenda.span`). */
function agendaSpan(today: string): number {
  const d = parseDay(today);
  const last = monthGrid(d.getFullYear(), d.getMonth() + 1)[41];
  return Math.round((last.getTime() - d.getTime()) / 86_400_000) + 1;
}

/** What the agenda widgets list: what is due in the folders “接下来” lists, as the `agenda` tool gives
 * it; with whether each can be done now, and the times a repeating one was done, for “一周” to tick
 * them off. */
export function widgetAgenda(board: Board, folders: (FolderId | null)[]) {
  const { overdue, days } = agenda(board, agendaSpan(board.now.today));
  const listed = (t: Task) => folders.some((f) => board.inSubtree(t.folder_id, f));
  const entry = (t: Task, later: boolean) => ({
    id: t.id,
    title: t.title,
    folder: board.folderPath(t.folder_id),
    due: t.due,
    completions: t.completions,
    status: board.status(t),
    ...(later && { later }),
  });
  return {
    overdue: overdue.filter(listed).map((t) => entry(t, false)),
    days: days
      .map(([date, list]) => ({ date, tasks: list.filter((d) => listed(d.task)).map((d) => entry(d.task, d.later)) }))
      .filter((d) => d.tasks.length > 0),
  };
}

/** How many days the widgets get written with them: from the first of this month's six weeks, a year
 * and more, which lasts them until the app is next opened. */
const LABELLED = 400;

/** The first of `today`'s month's six weeks, from which the widgets are given what is written with the days. */
function labelledFrom(today: string): string {
  const d = parseDay(today);
  return isoDate(monthGrid(d.getFullYear(), d.getMonth())[0]);
}

/** What the widgets write with each day (docs/design/repeat.md#节日和放假), `from` on: its festival, solar term
 * or lunar day, and with `marks`, whether it is a day off or a made-up working day. */
function widgetDays(from: string, marks: boolean) {
  const start = parseDay(from);
  const days = Array.from({ length: LABELLED }, (_, i) => {
    const date = isoDate(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i));
    const { text, note } = dayLabel(date);
    const h = marks ? holiday(date) : undefined;
    return { text, ...(note && { note }), ...(h && { holiday: h }) };
  });
  return { from, days };
}

/** What the device reminds of (docs/android/reminders.md): every reminder, past ones too, with its
 * time should the task repeat, whether the task waits for prerequisites as the reminder comes (a repeating one may
 * hold it up again by then), and for its start, which the device reads by its own clock. */
export const deviceReminders = (board: Board, hide: boolean, now = Date.now()) =>
  reminders(board, hide).map((r) => ({
    task: r.task.id,
    at: r.at.getTime(),
    occurrence: r.occurrence,
    completable: r.completable,
    title: r.title,
    body: r.body,
    // By the clock, not the board's moment, which may lag behind it.
    waiting: board.waitingOn(r.task, nowOf(new Date(Math.max(r.at.getTime(), now)))),
    start: r.task.start,
    start_time: r.task.start_time,
  }));

/** The daily digests the device shows (docs/android/reminders.md), none while off, and whether they
 * hide what they are of: the device says them, as `digestText` does, of what is left by then. */
export const deviceDigests = (board: Board, { on, time }: Digest, hide: boolean) => ({
  hide,
  list: on ? digests(board, time).map((d) => ({ at: d.at.getTime(), tasks: d.tasks })) : [],
});

/** What the system calendar shows when it is written to: the open tasks with a deadline in `folders`
 * (the root is all of them), and where each is. */
export const calendarEvents = (board: Board, folders: (FolderId | null)[]) =>
  board.snapshot.tasks.flatMap((t) =>
    t.done_at === null && t.due && board.mine(t) && folders.some((f) => board.inSubtree(t.folder_id, f))
      ? [{ task: t.id, title: t.title, due: t.due, description: t.folder_id ? `${board.place(t.folder_id)}\n来自 NextTodo` : "来自 NextTodo" }]
      : [],
  );

/** Writes the calendar for the session being watched: none once it has stopped, so that nothing
 * asked of a session that has ended, such as while the permission prompt waited, writes its tasks. */
let writeCalendar: (() => Promise<void>) | undefined;

/** Runs the session's update waiting to settle, if one is. */
let updateNow: (() => void) | undefined;

/** Brings the reminders, widgets and calendar up to date at once, not when the changes settle: the
 * quick add card closes after adding, its page with it. */
export async function settle() {
  updateNow?.();
  await queue;
}

/** Writes the tasks with a deadline to `target` from now on, or nowhere without one. */
export const setCalendar = (target: CalendarTarget | undefined) =>
  inTurn(async () => {
    await NextTodo.setCalendar({ target });
    await writeCalendar?.();
  });

/** The folders, each with those inside it, whose tasks the calendar gets on this phone, by user: the
 * root, which is all of them, until others are picked; one that has gone matches nothing. Kept on signing out, as the calendar
 * is: signing in again would otherwise write the folders left out. */
const FOLDERS = "calendar-folders:";

export const calendarFolders = (userId: string): (FolderId | null)[] => JSON.parse(localStorage.getItem(FOLDERS + userId) ?? "[null]");

export function setCalendarFolders(userId: string, folders: (FolderId | null)[]) {
  localStorage.setItem(FOLDERS + userId, JSON.stringify(folders));
  return inTurn(async () => await writeCalendar?.());
}

/** Keeps the reminders, and on Android the widget and the calendar, in step with the local copy of
 * `user`'s data, also when the app comes back to the front: the time has moved on, and the
 * permissions may have changed.
 * Takes in the changes made by the widget and reminders meanwhile. An update that fails is reported to
 * `failed`; the next change tries again. Returns how to stop and clear them. */
export function watch(store: Store, user: string, failed: (e: unknown) => void): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  // What the widgets were last given to write with the days: from which day, and whether marking days off.
  let labelled: { from: string; marks: boolean } | undefined;
  const update = async () => {
    // With the change the copy is as of, read together before any wait: the widgets keep lists they
    // fetched since, should the copy be behind the server.
    const { board } = store.getView();
    const seq = store.since;
    const start = board.nextStart();
    const nextStart = start ? [start.date, start.time].filter(Boolean).join(" ") : undefined;
    if (app === "android") {
      const listed = nextFolders(board);
      // Their paths, for the widget to fetch its list by itself (docs/android/widgets.md).
      const folders = listed.includes(null) ? undefined : JSON.stringify(listed.map((f) => board.folderPath(f)));
      await NextTodo.updateWidget({ tasks: JSON.stringify(widgetTasks(board, listed)), seq, nextStart, folders });
      await NextTodo.updateAgenda({ agenda: JSON.stringify(widgetAgenda(board, listed)), seq });
      const from = labelledFrom(board.now.today);
      const marks = prefsOf(board).holidays !== false;
      // Not the quick add card, whose copy may be behind the app's: the days last the widgets 400 days.
      if (!inCard && (labelled?.from !== from || labelled.marks !== marks)) {
        await NextTodo.updateDays({ days: JSON.stringify(widgetDays(from, marks)) });
        labelled = { from, marks };
      }
    }
    const hide = hideReminders();
    await NextTodo.setReminders({ reminders: JSON.stringify(deviceReminders(board, hide)), digest: JSON.stringify(deviceDigests(board, dailyDigest(), hide)), seq });
  };
  const calendar = () => NextTodo.updateCalendar({ events: JSON.stringify(calendarEvents(store.getView().board, calendarFolders(user))) });
  const takeOver = async () => {
    const { changes } = await NextTodo.takeChanges({ user });
    store.adopt(JSON.parse(changes) as Mutation[]);
  };
  // The calendar in a turn of its own: failing, it holds back neither the reminders nor the widget.
  const run = () => {
    inTurn(update).catch(failed);
    if (app === "android") inTurn(calendar).catch(failed);
  };
  const now = () => {
    clearTimeout(timer);
    timer = undefined;
    run();
  };
  const changed = () => {
    timer ??= setTimeout(now, SETTLE_MS);
  };
  const waiting = () => timer !== undefined && now();
  const shown = () => {
    if (document.visibilityState !== "visible") return;
    takeOver().catch(failed);
    changed();
  };
  const unsubscribe = store.subscribe(changed);
  const unpicked = onNextFolders(changed);
  const unset = onRemindSettings(changed);
  document.addEventListener("visibilitychange", shown);
  if (app === "android") writeCalendar = calendar;
  updateNow = waiting;
  takeOver().catch(failed);
  run();
  return async () => {
    // The next session may have started watching already.
    if (writeCalendar === calendar) writeCalendar = undefined;
    if (updateNow === waiting) updateNow = undefined;
    unsubscribe();
    unpicked();
    unset();
    document.removeEventListener("visibilitychange", shown);
    clearTimeout(timer);
    await clear(failed);
  };
}

/** Takes away every reminder, those shown already too, and on Android empties the widgets and deletes
 * the events written in the calendar; after the updates under way. The calendar failing goes to
 * `failed` and holds back neither the rest nor the sign-out: what is left there stays noted, and the
 * next session's first update deletes it, reporting should it fail again. Before a session there is no one to tell, and
 * that update alone reports. */
export const clear = async (failed: (e: unknown) => void = () => {}) => {
  if (app !== "android") return await inTurn(() => NextTodo.clearReminders());
  const calendar = inTurn(() => NextTodo.clearCalendar()).catch(failed);
  await inTurn(async () => {
    await NextTodo.updateWidget({ tasks: "[]" });
    await NextTodo.updateAgenda({ agenda: JSON.stringify({ overdue: [], days: [] }) });
    await NextTodo.clearReminders();
  });
  await calendar;
};

/** Signed out: the changes the widget and reminders could not send yet go. A session that just ended
 * keeps them for the same user's next sign-in. */
export const forget = () => NextTodo.forgetChanges();

/** Opens what a notification or a widget points at: `nexttodo://task/<id>`, `nexttodo://home`,
 * `nexttodo://calendar`, or its month or time grid on a day, `nexttodo://calendar/time/2026-11-26`; or
 * the quick add card, `nexttodo://page/<path>`, another page. */
function open(url: string) {
  const [, kind, id] = /^nexttodo:\/\/(task|home|calendar|page)\/?(.*)$/.exec(url) ?? [];
  if (kind === "task") navigate(`/?open=${id}`);
  else if (kind === "home") navigate("/");
  else if (kind === "calendar") {
    const [, view, on] = /^(time|month)?\/?(\d{4}-\d{2}-\d{2})?$/.exec(id) ?? [];
    // A day there is, from another app too.
    const day = on && isoDate(parseDay(on)) === on ? `&on=${on}` : "";
    navigate(view ? `/calendar?view=${view}${day}` : "/calendar");
  } else if (kind === "page") navigate(`/${id}`);
}

/** Listens for taps on notifications and the widget, and for Android's back button. The plugin keeps
 * the tap that started the app until there is a listener. */
export async function listen() {
  await App.addListener("appUrlOpen", (e) => open(e.url));
  await App.addListener("backButton", () => {
    // The quick add card's page closes it with a step of its own, once what was added is saved.
    if (!back()) void (inCard ? NextTodo.closeCard() : App.minimizeApp());
  });
}
