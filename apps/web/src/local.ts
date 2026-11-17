// Local mode in the apps: the server runs inside the app, for this device alone (docs/local.md). On
// the desktop the page starts it, the first time, and signs in to it each time it loads; in the phone
// apps the native side does both, and the page asks for what it got. Where it listens and the
// session's token are kept by the page nowhere.

import { Preferences } from "@capacitor/preferences";
import { invoke } from "@tauri-apps/api/core";
import { NextTodo } from "./mobile";
import { ApiError, api, app, connectLocal, dropSession, phoneApp } from "./api/http";
import type { DriveConfig } from "./api/types/DriveConfig";
import type { DriveTurnOn } from "./api/types/DriveTurnOn";
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

/** How the app's native part says the server did not start, or cannot be reached. */
type Rejected = { message: string; code?: string; locked?: boolean };

/** What the app's native part does for local mode: `apps/desktop/src-tauri/src/local.rs`, and
 * `NextTodoPlugin` in `apps/android` and `apps/ios`. */
const shell = phoneApp
    ? {
        // Capacitor's, which the app reads as it starts.
        mode: async () => (await Preferences.get({ key: "mode" })).value === "local",
        setMode: (on: boolean) => NextTodo.setLocalMode({ on }),
        start: () => NextTodo.openLocal(),
        reason: (e: Rejected): StartFailed["reason"] => (e.code === "locked" || e.code === "blocked" ? e.code : null),
        backup: async (name: string) => (await NextTodo.backupLocal({ name })).saved ?? null,
        prepareRestore: async () => (await NextTodo.prepareRestore()).picked,
        commitRestore: () => NextTodo.commitRestore(),
        restart: () => NextTodo.restart(),
      }
    : {
        mode: () => invoke<boolean>("local_mode"),
        setMode: (on: boolean) => invoke<void>("set_local_mode", { on }),
        start: (renew: boolean) => invoke<Started>("start_local", { renew }),
        reason: (e: Rejected): StartFailed["reason"] => (e.locked ? "locked" : null),
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

/** The server did not start, or the page may not reach it: `locked` while an earlier process of the app
 * still has its data, `blocked` while the system keeps the app off the network (Android). Trying again
 * may help with either. */
export class StartFailed extends Error {
  constructor(
    message: string,
    readonly reason: "locked" | "blocked" | null,
  ) {
    super(message);
  }
}

/** Starts the server, and signs the page in: on the desktop with the session the app's windows share,
 * a new one when told to `renew` it. */
async function start(renew: boolean): Promise<Started> {
  try {
    return await shell.start(renew);
  } catch (e) {
    const rejected = e as Rejected;
    throw new StartFailed(rejected.message, shell.reason(rejected));
  }
}

/** The local user's session, as the page loads in local mode; a new one when trying again after it
 * failed to open, which the one kept may have been the cause of. */
export const resume = async (renew = false) => open(await start(renew));

/** Enters local mode from the sign-in page, also when in it already, after the session ended say.
 * Changes of another account not sent yet would be lost: `confirm` is asked first, and null returned
 * when it declines. In the phone apps, the app restarts into it. */
export async function enter(confirm: (unsent: number) => Promise<boolean>): Promise<Session | null> {
  // The phone apps' open the data as they start; their server starts here already, to say whose it
  // is. The session the page had, if any, has ended.
  const started = phoneApp ? null : await start(true);
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
 * app does. In the phone apps the session goes with it, the local server's, or an earlier one's. */
export async function forgetMode() {
  if (mode) await setMode(false);
}

/** Leaves local mode, keeping the local user's data on the device for coming back, and restarts the
 * app, at the sign-in page: the app's server, running maybe even when the data would not open, stops
 * only with the app. `session`, when one is open, ends first. */
export async function leave(session?: Session) {
  await forgetMode();
  await session?.close(true);
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
  if (!(await sent(store))) throw new Error("备份失败：还有改动尚未保存，请稍后重试");
  return shell.backup(`NextTodo-备份-${isoDate(new Date())}.tar`);
}

/** How long turning syncing on or off may take: checking the drive, or downloading its data, or the
 * files of the attachments in use. */
const SETTING_MS = 10 * 60_000;

/** Turns syncing through the drive `config` says on (docs/local.md#开启). On a folder another device
 * syncs through, `confirm` is asked first; once it agreed, the data here is replaced with that, and
 * the app restarts. False when it declined. */
export async function syncThrough(config: DriveConfig, confirm: (body: string) => Promise<boolean>, session: Session): Promise<boolean> {
  try {
    await api<void>("PUT", "/local/sync", { config, replace: false } satisfies DriveTurnOn, SETTING_MS);
    return true;
  } catch (e) {
    if (!(e instanceof ApiError && e.code === "has_data")) throw e;
  }
  const body = [
    "网盘的这个文件夹里已经有别的设备同步的数据，开启后这台设备上的数据全部换成它，不合并。",
    "现在的数据会留在数据目录的旧数据文件夹里。",
    app === "desktop" ? "agent 用的令牌要重新生成。" : "",
    "NextTodo 会重新启动。",
  ].join("");
  if (!(await confirm(body))) return false;
  if (!(await sent(session.store))) throw new Error("开启失败：还有改动尚未保存，请稍后重试");
  await api<void>("PUT", "/local/sync", { config, replace: true } satisfies DriveTurnOn, SETTING_MS);
  // The drive's data replaces this at the next start, as a restore's does.
  try {
    await session.close(true);
  } finally {
    await shell.restart();
  }
  return true;
}

/** Stops syncing through the drive (docs/local.md#停止), once the files of the attachments in use are
 * downloaded; when they can't be now, `confirm` is asked whether to stop without them. False when it
 * declined. */
export async function stopSyncing(confirm: (body: string) => Promise<boolean>): Promise<boolean> {
  try {
    await api<void>("DELETE", "/local/sync", undefined, SETTING_MS);
    return true;
  } catch (e) {
    if (!(e instanceof ApiError && e.code === "files_not_here")) throw e;
    if (!(await confirm(e.message))) return false;
  }
  await api<void>("DELETE", "/local/sync?force=true", undefined, SETTING_MS);
  return true;
}

/** Asks for a backup and, once `confirm` has agreed to what goes, replaces the data with it and
 * restarts the app. From the settings, `session` is the one open: its changes are sent first, or
 * else lost. Nothing changes when the user cancels. */
export async function restore(confirm: (body: string) => Promise<boolean>, session?: Session) {
  if (session && !(await sent(session.store))) throw new Error("恢复失败：还有改动尚未保存，请稍后重试");
  if (!(await shell.prepareRestore())) return;
  const lost = session ? session.store.getView().pending : await unsent();
  const body = [
    lost ? `还没发出的 ${lost} 项改动会丢弃，找不回来。` : "",
    `现在的数据会留在数据目录的旧数据文件夹里；从现在到 NextTodo 重新启动之间，${app === "android" ? "小组件和 " : app === "desktop" ? "agent 和 " : ""}AI 整理写入的改动也只在那里。`,
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
