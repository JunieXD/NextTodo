// Who is signed in, and their local copy and connection. The app opens from the copy saved on this
// device without waiting for the network; the server is asked afterwards whether the session still
// holds.

import { createContext, useContext, useSyncExternalStore } from "react";
import { ApiError, api } from "./api/http";
import type { Effect } from "./api/types/Effect";
import type { ErrorBody } from "./api/types/ErrorBody";
import type { LogEntry } from "./api/types/LogEntry";
import type { Mutation } from "./api/types/Mutation";
import type { MutationId } from "./api/types/MutationId";
import type { Op } from "./api/types/Op";
import type { SeqSnapshot } from "./api/types/SeqSnapshot";
import type { User } from "./api/types/User";
import { Rejected } from "./domain/apply";
import { type Saver, forget, forgetCopy, forgetOthers, load, saver } from "./sync/persist";
import { type Connection, Socket } from "./sync/socket";
import { type Saved, Store } from "./sync/store";
import { useFeedback } from "./ui/feedback";

const LOGIN = "nexttodo.login";

/** The latest sign-in on this device, shared by its tabs: which one, so that a tab still in an
 * earlier one leaves it alone, and who signed in. */
interface Login {
  id: string;
  user: User;
}

function cachedLogin(): Login | null {
  const cached = localStorage.getItem(LOGIN);
  return cached && JSON.parse(cached);
}

/** Set while a sign-out has not reached the server: the session lives on there, and its cookie must
 * not sign the app back in. Finishing it also deletes what the sign-out may have left on the device,
 * had the tab been closed before it was done. */
const SIGNED_OUT = "nexttodo.signedOut";

/** Ends the browser's session on the server. When the server can't be reached, or fails, that is
 * done the next time the app opens. */
async function logout() {
  try {
    await api("POST", "/auth/logout");
  } catch (e) {
    // A 401 means it has ended already.
    if (!(e instanceof ApiError && e.status === 401)) return localStorage.setItem(SIGNED_OUT, "1");
  }
  localStorage.removeItem(SIGNED_OUT);
}

/** What a tab tells the others: that a sign-in ended, so that its other tabs end too instead of
 * saving its data again; that someone signed in, so that tabs of anyone else end, and the user's own
 * join the sign-in; that the server turned a change down, which only the tab that sent it hears, so
 * that tabs that took it from the outbox drop it too. */
type Message = { ended: string } | { signedIn: Login } | { rejected: MutationId };
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
  /** The session has ended elsewhere, and here too now. */
  signedOut(): void;
}

export class Session {
  readonly store: Store;
  private readonly saver: Saver;
  private readonly socket: Socket;
  connection: Connection = "connecting";
  private readonly watchers = new Set<() => void>();
  private listeners: Listeners = { rejected() {}, arrived() {}, unsaved() {}, signedOut() {} };
  /** How the session has ended here, once it has: when that is done, and whether the user's data
   * stays on this device. */
  private ended?: { done: Promise<void>; keep: boolean };
  private signingOut = false;

  constructor(
    public user: User,
    /** The sign-in this session belongs to. */
    private login: string,
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
      url: `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/api/v1/sync`,
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
  }

  private readonly heard = ({ data }: MessageEvent<Message>) => {
    if ("rejected" in data) this.store.drop(data.rejected);
    else if ("ended" in data ? data.ended === this.login : data.signedIn.user.id !== this.user.id) this.endedElsewhere(false);
    // Signed in as the same user again: the cookie is that sign-in's now, and so is this tab, unless
    // it is leaving.
    else if ("signedIn" in data && !this.ended) this.login = data.signedIn.id;
  };

  /** Ends the session here after it ended elsewhere, and tells the UI. When the server just stopped
   * accepting it, say because it was signed out on another device, the user's data stays on this
   * device, changes not sent yet included, for when they sign in again (`keep`). */
  private endedElsewhere(keep: boolean) {
    // Signing out here brings the server's word that the session has ended: that is no news.
    if (this.signingOut) return;
    void this.end(keep);
    // Now rather than once that is done: changes made meanwhile would be neither saved nor sent.
    this.listeners.signedOut();
  }

  listen(listeners: Listeners) {
    this.listeners = listeners;
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
    // end this sign-in nor clear its cookie.
    const { login, saved } = await turn(async () => claim(await request(), copy));
    return start(login, copy, saved);
  }

  /** The session this device was in last time, if any. */
  static async resume(): Promise<Session | null> {
    const copy = crypto.randomUUID();
    const found = await turn(async () => {
      if (localStorage.getItem(SIGNED_OUT)) {
        // Signed out last time while the server could not hear it: finish that, rather than be
        // signed in again by the cookie or opened from what is left.
        const login = cachedLogin();
        if (login) {
          await forget(login.user.id);
          localStorage.removeItem(LOGIN);
          tabs.postMessage({ ended: login.id } satisfies Message);
        }
        await logout();
        return null;
      }
      const login = cachedLogin();
      const saved = login && (await load(login.user.id, copy));
      if (saved) return { login, saved };
      // Nothing to open: ask the server who is signed in, in turn too, so that the answer still
      // holds when acted on.
      try {
        return await claim(await api<User>("GET", "/me"), copy);
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) return null;
        throw e;
      }
    });
    return found && start(found.login, copy, found.saved);
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

  /** Ends the session here, and in the other tabs in the same sign-in, and forgets the user's data
   * on this device. When the device has signed in as the same user again since, here or in another
   * tab, the data is that sign-in's now: only this session's copy goes. */
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
    await this.saver.close();
  }

  /** Deletes what the session leaves on this device, unless `keep`; in `turn`. Until then the other
   * tabs are heard, so that the sign-in ending elsewhere has the data deleted after all. */
  private async leave(keep: boolean) {
    tabs.removeEventListener("message", this.heard);
    const latest = cachedLogin();
    const current = latest?.id === this.login;
    if (latest && !current && latest.user.id === this.user.id) {
      // Signed in as the same user again since: the data is that sign-in's now.
      if (!keep) await forgetCopy(this.user.id, this.copy);
    } else if (!keep || (latest && !current)) {
      // Kept for the same user only: someone else's sign-in deleted it, but this session may have
      // saved some again since.
      await forget(this.user.id);
    }
    // The sign-in goes last: a sign-out cut short before has no data left to open with.
    if (current) localStorage.removeItem(LOGIN);
    // To the sign-in's other tabs, whether or not it is still the latest.
    if (!keep) tabs.postMessage({ ended: this.login } satisfies Message);
  }

  /** Signs out on the server, unless the browser has signed in again since in another tab: the
   * cookie is that sign-in's now. Then ends the session as `close` does. */
  async signOut() {
    this.signingOut = true;
    await turn(async () => {
      if (cachedLogin()?.id === this.login) await logout();
    });
    await this.close();
  }
}

/** Makes `user` the device's latest sign-in and loads their data; anyone else's goes. In `turn`. */
async function claim(user: User, copy: string) {
  const login: Login = { id: crypto.randomUUID(), user };
  localStorage.setItem(LOGIN, JSON.stringify(login));
  localStorage.removeItem(SIGNED_OUT);
  tabs.postMessage({ signedIn: login } satisfies Message);
  await forgetOthers(user.id);
  return { login, saved: await load(user.id, copy) };
}

/** A session in `login`, from the user's data saved on this device, or else a fresh copy from the
 * server. */
async function start({ id, user }: Login, copy: string, saved: Saved | undefined) {
  if (saved) return new Session(user, id, copy, saved);
  const { seq, folders, tasks, dependencies } = await download(user.id);
  return new Session(user, id, copy, { ...Store.empty(), seq, base: { folders, tasks, dependencies } });
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
