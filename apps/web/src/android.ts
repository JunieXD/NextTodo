// What only the Android app does: deadline reminders scheduled on the device, and the list the
// home-screen widget shows. See docs/android.md.

import { App } from "@capacitor/app";
import { registerPlugin } from "@capacitor/core";
import { LocalNotifications, type PendingLocalNotificationSchema } from "@capacitor/local-notifications";
import { navigate } from "wouter/use-browser-location";
import type { Board } from "./domain/board";
import { type Reminder, reminder, reminders } from "./domain/reminders";
import type { Store } from "./sync/store";

/** The app's own plugin, in `apps/android`. */
interface NextTodoPlugin {
  /** Hands the widget its list, as JSON in the shape of the `next` tool's data. */
  updateWidget(o: { tasks: string }): Promise<void>;
  /** The system settings page of this app, where autostart and battery use are allowed. */
  openAppSettings(): Promise<void>;
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
  board.next().map((t) => ({ id: t.id, number: t.number, title: t.title, folder: board.folderPath(t.folder_id), due: t.due }));

/** Keeps the reminders and the widget in step with the local copy, also when the app comes back to
 * the front: the time has moved on, and the permissions may have changed. An update that fails is
 * reported to `failed`; the next change tries again. Returns how to stop and clear both. */
export function watch(store: Store, failed: (e: unknown) => void): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const update = async () => {
    const { board } = store.getView();
    await NextTodo.updateWidget({ tasks: JSON.stringify(widgetTasks(board)) });
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
  const changed = () => {
    timer ??= setTimeout(() => {
      timer = undefined;
      inTurn(update).catch(failed);
    }, SETTLE_MS);
  };
  const shown = () => document.visibilityState === "visible" && changed();
  const unsubscribe = store.subscribe(changed);
  document.addEventListener("visibilitychange", shown);
  inTurn(update).catch(failed);
  return async () => {
    unsubscribe();
    document.removeEventListener("visibilitychange", shown);
    clearTimeout(timer);
    await clear();
  };
}

/** Empties the widget and takes away every reminder, those shown already too; after the updates
 * under way. */
export const clear = () =>
  inTurn(async () => {
    await NextTodo.updateWidget({ tasks: "[]" });
    const { notifications } = await LocalNotifications.getPending();
    if (notifications.length) await LocalNotifications.cancel({ notifications: notifications.map((n) => ({ id: n.id })) });
    await LocalNotifications.removeAllDeliveredNotifications();
  });

/** Opens what a notification or the widget points at: `nexttodo://task/<id>`, `nexttodo://add` or
 * `nexttodo://home`. */
function open(url: string) {
  const [, kind, id] = /^nexttodo:\/\/(task|add|home)\/?(.*)$/.exec(url) ?? [];
  if (kind === "task") navigate(`/?t=${id}`, { state: { sheet: true } });
  else if (kind === "add") navigate("/?add");
  else if (kind === "home") navigate("/");
}

/** Listens for taps on notifications and the widget. The plugins keep the one that started the app
 * until there is a listener. */
export async function listen() {
  await LocalNotifications.addListener("localNotificationActionPerformed", (e) => open(`nexttodo://task/${e.notification.extra.task}`));
  await App.addListener("appUrlOpen", (e) => open(e.url));
  // Back goes back in the app, and from the home screen leaves it, as elsewhere on Android. The web
  // view can't go back over pages opened without a tap, such as from a notification: those go home.
  await App.addListener("backButton", ({ canGoBack }) => {
    if (canGoBack) history.back();
    else if (location.pathname !== "/" || location.search) navigate("/", { replace: true });
    else void App.minimizeApp();
  });
}
