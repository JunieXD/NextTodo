// Local mode in the desktop app: the server runs inside the app, for this device alone (docs/local.md).
// Each time the page loads it starts the server, the first time, and signs in to it; where it
// listens and the session's token are kept nowhere.

import { invoke } from "@tauri-apps/api/core";
import { api, app, connectLocal, dropSession } from "./api/http";
import type { User } from "./api/types/User";
import { Session } from "./session";
import { forgetSynced, unsentOfOthers } from "./sync/persist";

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
