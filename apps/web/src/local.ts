// Local mode in the apps: the server runs inside the app, for this device alone (docs/local.md). On
// the desktop the page starts it, the first time, and signs in to it each time it loads; on Android
// the app does both as it starts, and the page asks for what it got. Where it listens and the
// session's token are kept by the page nowhere.

import { Preferences } from "@capacitor/preferences";
import { invoke } from "@tauri-apps/api/core";
import { NextTodo } from "./android";
import { api, app, connectLocal, dropSession } from "./api/http";
import type { User } from "./api/types/User";
import { isoDate } from "./domain/dates";
import { Session } from "./session";
import { forgetSynced, unsent, unsentOfOthers } from "./sync/persist";
import type { Store } from "./sync/store";

/** The epoch of the database the local user's data on this device came from. Kept on leaving local
 * mode, as the data is, for coming back. Should it be lost, the data is downloaded again. */
const EPOCH = "nexttodo.localEpoch";

/** What the page needs to open the local user's data. */
export interface Started {
  url: string;
  token: string;
  user: string;
  epoch: string;
}

/** How the app's native part says the server did not start. */
type Rejected = { message: string; code?: string; locked?: boolean };

/** What the app's native part does for local mode: `apps/desktop/src-tauri/src/local.rs`, and
 * `NextTodoPlugin` in `apps/android`. */
const shell =
  app === "android"
    ? {
        // Capacitor's, which the app reads as it starts.
        mode: async () => (await Preferences.get({ key: "mode" })).value === "local",
        setMode: (on: boolean) => NextTodo.setLocalMode({ on }),
        start: () => NextTodo.openLocal(),
        locked: (e: Rejected) => e.code === "locked",
        backup: async (name: string) => (await NextTodo.backupLocal({ name })).saved ?? null,
        prepareRestore: async () => (await NextTodo.prepareRestore()).picked,
        commitRestore: () => NextTodo.commitRestore(),
        restart: () => NextTodo.restart(),
      }
    : {
        mode: () => invoke<boolean>("local_mode"),
        setMode: (on: boolean) => invoke<void>("set_local_mode", { on }),
        start: () => invoke<Started>("start_local"),
        locked: (e: Rejected) => !!e.locked,
        backup: (name: string) => invoke<string | null>("backup_local", { name }),
        prepareRestore: () => invoke<boolean>("prepare_restore"),
        commitRestore: () => invoke<void>("commit_restore"),
        restart: () => invoke<void>("restart"),
      };

/** Whether the app is in local mode, as the app keeps it. */
let mode = false;

/** Reads whether the app is in local mode. Call before `inLocalMode`. */
export async function loadMode() {
  if (app) mode = await shell.mode();
}

export const inLocalMode = () => mode;

async function setMode(on: boolean) {
  await shell.setMode(on);
  mode = on;
}

/** The server did not start; `locked` while an earlier process of the app still has its data. */
export class StartFailed extends Error {
  constructor(
    message: string,
    readonly locked: boolean,
  ) {
    super(message);
  }
}

async function start(): Promise<Started> {
  try {
    return await shell.start();
  } catch (e) {
    const rejected = e as Rejected;
    throw new StartFailed(rejected.message, shell.locked(rejected));
  }
}

/** The local user's session, as the page loads in local mode. */
export const resume = async () => open(await start());

/** Enters local mode from the sign-in page, also when in it already, after the session ended say.
 * Changes of another account not sent yet would be lost: `confirm` is asked first, and null returned
 * when it declines. On Android, the app restarts into it. */
export async function enter(confirm: (unsent: number) => Promise<boolean>): Promise<Session | null> {
  // Android's opens the data as it starts; its server starts here already, to say whose it is.
  const started = app === "android" ? null : await start();
  const unsent = await unsentOfOthers(started?.user ?? (await NextTodo.startLocal()).user);
  if (unsent && !(await confirm(unsent))) return null;
  // Should a server's token be kept, it never comes back: after local mode the app signs in afresh.
  await dropSession();
  await setMode(true);
  if (started) return open(started);
  await shell.restart();
  return null;
}

/** Opens the local user's data, as signing in does: anyone else's on the page goes. */
function open({ url, token, user, epoch }: Started): Promise<Session> {
  connectLocal(url, token);
  return Session.open(async () => {
    // From a database with another history, a backup restored say, the copy and the changes not sent
    // yet would not fit it; the drafts and the AI settings stay. Deleted before the epoch is noted,
    // so that if cut short, they are the next time.
    if (localStorage.getItem(EPOCH) !== epoch) {
      await forgetSynced(user);
      localStorage.setItem(EPOCH, epoch);
    }
    return api<User>("GET", "/me");
  });
}

/** Forgets local mode, when in it, for signing in to a server: the app's own server stops when the
 * app does. On Android the session goes with it, the local server's, or an earlier one's. */
export async function forgetMode() {
  if (mode) await setMode(false);
}

/** Leaves local mode, keeping the local user's data on the device for coming back, and restarts the
 * app, at the sign-in page. */
export async function leave(session: Session) {
  await forgetMode();
  await session.close(true);
  await shell.restart();
}

/** How long a backup, or a restore from the settings, waits for the changes made here to reach the
 * app's server. It is right there: usually they have. */
const SEND_WAIT_MS = 5000;

/** Waits for the changes made here to reach the app's server; false when some have not in time. */
export function sent(store: Store): Promise<boolean> {
  return new Promise((resolve) => {
    const check = () => {
      if (store.getView().pending) return;
      clearTimeout(timer);
      stop();
      resolve(true);
    };
    const timer = setTimeout(() => (stop(), resolve(false)), SEND_WAIT_MS);
    const stop = store.subscribe(check);
    check();
  });
}

/** Asks where to save a backup and writes it there: where it went, or null when the user cancelled.
 * The changes made here are in it. */
export async function backup(store: Store): Promise<string | null> {
  if (!(await sent(store))) throw new Error("还有改动没有保存，请稍后再试");
  return shell.backup(`NextTodo-备份-${isoDate(new Date())}.db`);
}

/** Asks for a backup and, once `confirm` has agreed to what goes, replaces the data with it and
 * restarts the app. From the settings, `session` is the one open: its changes are sent first, or
 * else lost. Nothing changes when the user cancels. */
export async function restore(confirm: (body: string) => Promise<boolean>, session?: Session) {
  if (session && !(await sent(session.store))) throw new Error("还有改动没有保存，请稍后再试");
  if (!(await shell.prepareRestore())) return;
  const lost = session ? session.store.getView().pending : await unsent();
  const body = [
    lost ? `还没发出的 ${lost} 项改动会丢弃，找不回来。` : "",
    `现在的数据会留在数据目录的旧数据文件夹里；从现在到 NextTodo 重新启动之间，${app === "android" ? "小组件" : "agent "}和 AI 整理写入的改动也只在那里。`,
  ].join("");
  if (!(await confirm(body))) return;
  // Should it fail, nothing has changed, and the session carries on.
  await shell.commitRestore();
  // Committed: the app restarts whatever else happens. Nothing more goes to the data being replaced;
  // what is on the page stays, as when leaving local mode: with the backup's epoch, it is deleted then.
  try {
    await session?.close(true);
  } finally {
    await shell.restart();
  }
}
