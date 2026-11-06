// Local mode in the desktop app: the server runs inside the app, for this device alone (docs/local.md).
// Each time the page loads it starts the server, the first time, and signs in to it; where it
// listens and the session's token are kept nowhere.

import { invoke } from "@tauri-apps/api/core";
import { api, app, connectLocal, dropSession } from "./api/http";
import type { User } from "./api/types/User";
import { isoDate } from "./domain/dates";
import { Session } from "./session";
import { forgetSynced, unsent, unsentOfOthers } from "./sync/persist";
import type { Store } from "./sync/store";

/** The epoch of the database the local user's data on this device came from. Kept on leaving local
 * mode, as the data is, for coming back. Should it be lost, the data is downloaded again. */
const EPOCH = "nexttodo.localEpoch";

/** Whether the app is in local mode, as the app keeps it (`set_local_mode`). */
let mode = false;

/** Reads whether the app is in local mode. Call before `inLocalMode`. */
export async function loadMode() {
  if (app === "desktop") mode = await invoke<boolean>("local_mode");
}

export const inLocalMode = () => mode;

async function setMode(on: boolean) {
  await invoke("set_local_mode", { on });
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

/** What `start_local` gives (`apps/desktop/src-tauri/src/local.rs`). */
interface Started {
  url: string;
  token: string;
  user: string;
  epoch: string;
}

async function start(): Promise<Started> {
  try {
    return await invoke<Started>("start_local");
  } catch (e) {
    const { message, locked } = e as { message: string; locked: boolean };
    throw new StartFailed(message, locked);
  }
}

/** The local user's session, as the page loads in local mode. */
export const resume = async () => open(await start());

/** Enters local mode from the sign-in page, also when in it already, after the session ended say.
 * Changes of another account not sent yet would be lost: `confirm` is asked first, and null returned
 * when it declines. */
export async function enter(confirm: (unsent: number) => Promise<boolean>): Promise<Session | null> {
  const started = await start();
  const unsent = await unsentOfOthers(started.user);
  if (unsent && !(await confirm(unsent))) return null;
  // Should a server's token be kept, it never comes back: after local mode the app signs in afresh.
  await dropSession();
  await setMode(true);
  return open(started);
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

/** Forgets local mode, for signing in to a server: the app's own server stops when the app does. */
export const forgetMode = () => setMode(false);

/** Leaves local mode, keeping the local user's data on the device for coming back, and restarts the
 * app, at the sign-in page. */
export async function leave(session: Session) {
  await forgetMode();
  await session.close(true);
  await invoke("restart");
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
  return invoke<string | null>("backup_local", { name: `NextTodo-备份-${isoDate(new Date())}.db` });
}

/** Asks for a backup and, once `confirm` has agreed to what goes, replaces the data with it and
 * restarts the app. From the settings, `session` is the one open: its changes are sent first, or
 * else lost. Nothing changes when the user cancels. */
export async function restore(confirm: (body: string) => Promise<boolean>, session?: Session) {
  if (session && !(await sent(session.store))) throw new Error("还有改动没有保存，请稍后再试");
  if (!(await invoke<boolean>("prepare_restore"))) return;
  const lost = session ? session.store.getView().pending : await unsent();
  const body = [
    lost ? `还没发出的 ${lost} 项改动会丢弃，找不回来。` : "",
    "现在的数据会留在数据目录的旧数据文件夹里；从现在到 NextTodo 重新启动之间，agent 和 AI 整理写入的改动也只在那里。",
  ].join("");
  if (!(await confirm(body))) return;
  // Nothing more goes to the data being replaced. What is on the page stays, as when leaving local
  // mode: with the backup's epoch, it is deleted then.
  await session?.close(true);
  await invoke("commit_restore");
  await invoke("restart");
}
