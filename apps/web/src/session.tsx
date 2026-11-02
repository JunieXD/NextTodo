// Who is signed in, and their local copy and connection. The app opens from the copy saved on this
// device without waiting for the network; the server is asked afterwards whether the session still
// holds.

import { createContext, useContext, useSyncExternalStore } from "react";
import { ApiError, api, app, dropSession, native, syncToken, syncUrl } from "./api/http";
import type { Effect } from "./api/types/Effect";
import type { ErrorBody } from "./api/types/ErrorBody";
import type { LogEntry } from "./api/types/LogEntry";
import type { Mutation } from "./api/types/Mutation";
import type { MutationId } from "./api/types/MutationId";
import type { Op } from "./api/types/Op";
import type { SeqSnapshot } from "./api/types/SeqSnapshot";
import type { User } from "./api/types/User";
import { Rejected } from "./domain/apply";
import * as android from "./android";
import * as macos from "./macos";
import { type Saver, forget, forgetCopy, forgetOthers, load, saver } from "./sync/persist";
import { type Connection, Socket } from "./sync/socket";
import { type Saved, Store } from "./sync/store";
import { useFeedback } from "./ui/feedback";

const LOGIN = "nexttodo.login";

/** The latest sign-in on this device, shared by its tabs: which one, so that a tab still in an
 * earlier one leaves it alone, and who signed in. */
interface Login {
  /** Its number in the device's sequence of sign-ins and sign-outs. */
  id: number;
  user: User;
}

const COUNT = "nexttodo.count";

/** The next number in the device's sequence of sign-ins and sign-outs; in `turn`. Never behind the
 * clock in milliseconds: should the site's storage be cleared, tabs left open have smaller ones. */
function next(): number {
  const n = Math.max(Number(localStorage.getItem(COUNT)) + 1, Date.now());
  localStorage.setItem(COUNT, String(n));
  return n;
}

function cachedLogin(): Login | null {
  const cached = localStorage.getItem(LOGIN);
  return cached && JSON.parse(cached);
}

/** Set from the start of a sign-out until it is done, on the server and on this device, to the sign-in
 * it ends: cut short, it is finished the next time the app opens, rather than the app being signed in
 * again by the cookie or opened from what is left. Whoever signs in next clears it. */
const SIGNED_OUT = "nexttodo.signedOut";

function pendingSignOut(): Login | null {
  const marked = localStorage.getItem(SIGNED_OUT);
  return marked && JSON.parse(marked);
}

/** Ends the browser's session on the server; whether that is done, or was already. */
async function logout(): Promise<boolean> {
  try {
    await api("POST", "/auth/logout");
  } catch (e) {
    // A 401 means it has ended already.
    return e instanceof ApiError && e.status === 401;
  }
  return true;
}

/** What a tab tells the others, numbered in the device's sequence: who signed in, so that tabs of
 * anyone else end; who signed out on this device, so that the user's tabs end too, whichever sign-in
 * they are in, instead of saving the data again; that the server turned a change down, which only
 * the tab that sent it hears, so that tabs that took it from the outbox drop it too. */
type Message = { signedIn: Login } | { signedOut: Login } | { rejected: MutationId };
const tabs = new BroadcastChannel("nexttodo");

/** Runs `f` while no other tab is signing in or out: what it finds on the device stays so until it
 * is done. */
const turn = <T,>(f: () => Promise<T>): Promise<T> => navigator.locks.request("nexttodo.login", f);

/** What the UI wants to hear about; set once it is on screen. */
export interface Listeners {
  rejected(m: Mutation, e: ErrorBody): void;
  arrived(entries: LogEntry[]): void;
  /** Saving on this device failed; changes still go to the server. */
  unsaved(): void;
  /** In the app: the reminders, or on Android the widget, could not be brought up to date. */
  appFailed(e: unknown): void;
  /** The session has ended elsewhere, and here too now. */
  signedOut(): void;
}

export class Session {
  readonly store: Store;
  private readonly saver: Saver;
  private readonly socket: Socket;
  /** In the app: stops keeping the reminders and the widget in step, and clears them. */
  private readonly unwatch?: () => Promise<void>;
  connection: Connection = "connecting";
  private readonly watchers = new Set<() => void>();
  private listeners: Listeners = { rejected() {}, arrived() {}, unsaved() {}, appFailed() {}, signedOut() {} };
  /** How the session has ended here, once it has: when that is done, and whether the user's data
   * stays on this device. */
  private ended?: { done: Promise<void>; keep: boolean };
  private signingOut = false;
  /** Whether signing out has ended the session on the server. */
  private loggedOut = false;

  /** Made in `turn`, so that it hears every sign-in and sign-out after what it was made from. */
  constructor(
    public user: User,
    /** The sign-in this session belongs to. */
    private readonly login: number,
    /** Its copy of the user's data on this device. */
    private readonly copy: string,
    saved: Saved,
  ) {
    this.saver = saver(user.id, copy, (e) => {
      console.error(e);
      this.listeners.unsaved();
    });
    this.store = new Store(saved, {
      save: (copy, queued, settled) => this.saver.save(copy, queued, settled),
      send: (m) => this.socket.send(m),
      rejected: (m, e) => {
        tabs.postMessage({ rejected: m.id } satisfies Message);
        this.listeners.rejected(m, e);
      },
      arrived: (entries) => this.listeners.arrived(entries),
    });
    this.socket = new Socket(this.store, {
      url: syncUrl(),
      token: syncToken(),
      user: user.id,
      resync: async () => this.store.resync(await download(user.id)),
      signedOut: () => this.endedElsewhere(true),
      status: (c) => {
        this.connection = c;
        for (const w of this.watchers) w();
      },
    });
    this.socket.start();
    tabs.addEventListener("message", this.heard);
    if (app) this.unwatch = (app === "android" ? android : macos).watch(this.store, (e) => this.listeners.appFailed(e));
  }

  private readonly heard = ({ data }: MessageEvent<Message>) => {
    if ("rejected" in data) this.store.drop(data.rejected);
    // A message can be heard late: only a sign-in or sign-out after this session's sign-in ends it.
    else if ("signedIn" in data) {
      if (data.signedIn.user.id !== this.user.id && data.signedIn.id > this.login) this.endedElsewhere(false);
    } else if (data.signedOut.user.id === this.user.id && data.signedOut.id > this.login) this.endedElsewhere(false);
  };

  /** Ends the session here after it ended elsewhere, and tells the UI. When the server just stopped
   * accepting it, say because it was signed out on another device, the user's data stays on this
   * device, changes not sent yet included, for when they sign in again (`keep`). */
  private endedElsewhere(keep: boolean) {
    // Signing out here brings word that the session has ended: that is no news.
    if (this.signingOut) return;
    void this.end(keep);
    // Now rather than once that is done: changes made meanwhile would be neither saved nor sent.
    this.listeners.signedOut();
  }

  /** Also tells the UI if the session has ended elsewhere before it listened. */
  listen(listeners: Listeners) {
    this.listeners = listeners;
    if (this.ended && !this.signingOut) listeners.signedOut();
  }

  setUser(user: User) {
    this.user = user;
    void turn(async () => {
      if (cachedLogin()?.id === this.login) localStorage.setItem(LOGIN, JSON.stringify({ id: this.login, user }));
    });
  }

  watchConnection = (w: () => void) => {
    this.watchers.add(w);
    return () => this.watchers.delete(w);
  };

  /** Signs in with `request`, which asks the server and says who signed in, and opens their data:
   * the copy on this device, or a fresh one from the server. Anyone else's data on this device goes. */
  static async open(request: () => Promise<User>): Promise<Session> {
    const copy = crypto.randomUUID();
    // The request sets the browser's cookie: in turn, so that a sign-out in another tab can neither
    // end this sign-in nor clear its cookie. One cut short goes first, while the cookie is its own.
    const opened = await turn(async () => {
      const out = pendingSignOut();
      if (out) await finishSignOut(out);
      return claim(await request(), copy);
    });
    return opened instanceof Session ? opened : fresh(opened, copy);
  }

  /** The session this device was in last time, if any. */
  static async resume(): Promise<Session | null> {
    // In the app, the session is the token kept on the device: without one, what the widget and the
    // reminders show is no one's. On the Mac they last only while the app runs.
    if (app === "android" && !syncToken()) await android.clear();
    const copy = crypto.randomUUID();
    const found = await turn(async () => {
      const out = pendingSignOut();
      if (out) {
        await finishSignOut(out);
        return null;
      }
      // No token without a sign-out to finish: the server stopped accepting the session, and the
      // widget heard so first. The data stays, as when the app hears so itself.
      if (native && !syncToken()) {
        localStorage.removeItem(LOGIN);
        return null;
      }
      const login = cachedLogin();
      const saved = login && (await load(login.user.id, copy));
      if (saved) return new Session(login.user, login.id, copy, saved);
      // Nothing to open: ask the server who is signed in, in turn too, so that the answer still
      // holds when acted on.
      try {
        return await claim(await api<User>("GET", "/me"), copy);
      } catch (e) {
        if (!(e instanceof ApiError && e.status === 401)) throw e;
        // In the app, the token goes too, as when a session ends.
        if (native) {
          await dropSession();
          if (app === "android") await android.clear();
        }
        return null;
      }
    });
    return !found || found instanceof Session ? found : fresh(found, copy);
  }

  /** Asks the server whether the session still holds, and refreshes the user's details. */
  async check() {
    let me: User;
    try {
      me = await api<User>("GET", "/me");
    } catch (e) {
      // Offline, or the server is down: carry on with the local copy.
      if (e instanceof ApiError && e.status === 401) this.endedElsewhere(true);
      return;
    }
    // Someone else signed in meanwhile, in another tab: this copy is not theirs.
    if (me.id !== this.user.id) this.endedElsewhere(false);
    else this.setUser(me);
  }

  /** Ends the session here, and forgets the user's data on this device. When the device has signed in
   * as the same user again since, here or in another tab, the data is that sign-in's now: only this
   * session's copy goes. */
  close(): Promise<void> {
    return this.end(false);
  }

  /** Ends the session here; `keep` keeps the user's data on this device. Ending it again without
   * `keep` deletes the data after all. */
  private end(keep: boolean): Promise<void> {
    if (this.ended && (keep || !this.ended.keep)) return this.ended.done;
    const stopped = this.ended?.done ?? this.stop();
    const done = stopped.then(() => turn(() => this.leave(keep)));
    this.ended = { done, keep };
    return done;
  }

  private async stop() {
    this.socket.stop();
    // The token goes first, so that the widget, emptied next, asks to sign in.
    await dropSession();
    await this.unwatch?.();
    await this.saver.close();
  }

  /** Deletes what the session leaves on this device, unless `keep`; in `turn`. Until then the other
   * tabs are heard, so that the user signing out elsewhere has the data deleted after all. */
  private async leave(keep: boolean) {
    tabs.removeEventListener("message", this.heard);
    const latest = cachedLogin();
    const current = latest?.id === this.login;
    if (latest && !current) {
      // Signed in again since: as the same user, the data is that sign-in's now; as someone else,
      // their sign-in deleted it, but this session may have saved some again since.
      if (latest.user.id !== this.user.id) await forget(this.user.id);
      else if (!keep) await forgetCopy(this.user.id, this.copy);
    } else if (!keep) await forgetUser(this.user);
    else if (current) localStorage.removeItem(LOGIN);
    // Signed out on the server too, and the data deleted: done.
    if (!keep && this.loggedOut && pendingSignOut()?.id === this.login) localStorage.removeItem(SIGNED_OUT);
  }

  /** Signs out on the server, unless the browser has signed in again since in another tab: the
   * cookie is that sign-in's now. Then ends the session as `close` does. */
  async signOut() {
    this.signingOut = true;
    await turn(async () => {
      if (cachedLogin()?.id !== this.login) return;
      localStorage.setItem(SIGNED_OUT, JSON.stringify({ id: this.login, user: this.user } satisfies Login));
      this.loggedOut = await logout();
    });
    await this.close();
  }
}

/** Makes `user` the device's latest sign-in and opens their data saved on the device; anyone
 * else's goes. In `turn`. Without data saved, returns the sign-in to download it for. */
async function claim(user: User, copy: string): Promise<Session | Login> {
  const login: Login = { id: next(), user };
  localStorage.setItem(LOGIN, JSON.stringify(login));
  localStorage.removeItem(SIGNED_OUT);
  tabs.postMessage({ signedIn: login } satisfies Message);
  await forgetOthers(user.id);
  const saved = await load(user.id, copy);
  return saved ? new Session(user, login.id, copy, saved) : login;
}

/** A session in `login` with a fresh copy from the server, unless the device has signed out, or in
 * as someone else, while it downloaded. */
async function fresh({ id, user }: Login, copy: string): Promise<Session> {
  const { seq, folders, tasks, dependencies } = await download(user.id);
  return turn(async () => {
    if (pendingSignOut() || cachedLogin()?.user.id !== user.id) throw new ApiError(401, "unauthenticated", "这台设备上的登录已经变了，请重新登录");
    return new Session(user, id, copy, { ...Store.empty(), seq, base: { folders, tasks, dependencies } });
  });
}

/** Signs the user out on this device: deletes their data, and their sign-in if it is still the
 * latest; their tabs still open end. In `turn`. */
async function forgetUser(user: User) {
  await forget(user.id);
  // The sign-in goes last: cut short before, there is no data left to open with.
  if (cachedLogin()?.user.id === user.id) localStorage.removeItem(LOGIN);
  tabs.postMessage({ signedOut: { id: next(), user } } satisfies Message);
}

/** Finishes a sign-out cut short; in `turn`. */
async function finishSignOut(out: Login) {
  await forgetUser(out.user);
  const loggedOut = await logout();
  // In the app, the token goes too, and then what the widget and the reminders show; on the Mac
  // reminders last only while the app runs.
  if (native) {
    await dropSession();
    if (app === "android") await android.clear();
  }
  if (loggedOut) localStorage.removeItem(SIGNED_OUT);
}

/** The user's data from the server, unless this browser has signed in as someone else by now. */
async function download(user: string): Promise<SeqSnapshot> {
  const snapshot = await api<SeqSnapshot>("GET", "/snapshot");
  if (snapshot.user !== user) throw new ApiError(401, "unauthenticated", "这台设备上登录的账号已经换了，请重新登录");
  return snapshot;
}

export const SessionContext = createContext<Session>(null!);

export const useSession = () => useContext(SessionContext);

/** The data to show, kept up to date. */
export function useView() {
  const { store } = useSession();
  return useSyncExternalStore(store.subscribe, store.getView);
}

/** Whether the server is reachable. */
export function useConnection(): Connection {
  const session = useSession();
  return useSyncExternalStore(session.watchConnection, () => session.connection);
}

/** Makes a change, telling the user when it cannot be made. Returns what changed, or `null`. */
export function useCommit() {
  const { store } = useSession();
  const { snack } = useFeedback();
  return (ops: Op[]): { id: MutationId; effects: Effect[] } | null => {
    try {
      return store.commit(ops);
    } catch (e) {
      if (!(e instanceof Rejected)) throw e;
      snack(e.message);
      return null;
    }
  };
}

/** Undoes a change and says so. */
export function useUndo() {
  const commit = useCommit();
  const { snack } = useFeedback();
  return (id: MutationId, said = "已撤销") => {
    if (commit([{ op: "undo", mutation: id }])) snack(said);
  };
}
