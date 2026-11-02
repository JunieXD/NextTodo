// Who is signed in, and their local copy and connection. The app opens from the copy saved on this
// device without waiting for the network; the server is asked afterwards whether the session still
// holds.

import { createContext, useContext, useSyncExternalStore } from "react";
import { ApiError, OFFLINE, api } from "./api/http";
import type { Effect } from "./api/types/Effect";
import type { ErrorBody } from "./api/types/ErrorBody";
import type { LogEntry } from "./api/types/LogEntry";
import type { Mutation } from "./api/types/Mutation";
import type { MutationId } from "./api/types/MutationId";
import type { Op } from "./api/types/Op";
import type { SeqSnapshot } from "./api/types/SeqSnapshot";
import type { User } from "./api/types/User";
import { Rejected } from "./domain/apply";
import { type Saver, forget, forgetCopy, load, saver } from "./sync/persist";
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

/** What a tab tells the others: that a sign-in ended, so that its other tabs end too instead of
 * saving its data again; that the server turned a change down, which only the tab that sent it
 * hears, so that tabs that took it from the outbox drop it too. */
type Message = { ended: string } | { rejected: MutationId };
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
  signedOut(): void;
}

export class Session {
  readonly store: Store;
  private readonly saver: Saver;
  private readonly socket: Socket;
  connection: Connection = "connecting";
  private readonly watchers = new Set<() => void>();
  private listeners: Listeners = { rejected() {}, arrived() {}, unsaved() {}, signedOut() {} };
  private closing?: Promise<void>;

  constructor(
    public user: User,
    /** The sign-in this session belongs to. */
    private readonly login: string,
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
      signedOut: () => this.listeners.signedOut(),
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
    else if (data.ended === this.login) this.listeners.signedOut();
  };

  listen(listeners: Listeners) {
    this.listeners = listeners;
  }

  /** The session has ended elsewhere. */
  ended() {
    this.listeners.signedOut();
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

  /** Signs in as `user`, from the copy on this device or a fresh one from the server. */
  static async open(user: User): Promise<Session> {
    const login = crypto.randomUUID();
    const copy = crypto.randomUUID();
    const saved = await turn(async () => {
      localStorage.setItem(LOGIN, JSON.stringify({ id: login, user }));
      return load(user.id, copy);
    });
    if (saved) return new Session(user, login, copy, saved);
    const { seq, folders, tasks, dependencies } = await download(user.id);
    return new Session(user, login, copy, { ...Store.empty(), seq, base: { folders, tasks, dependencies } });
  }

  /** The session this device was in last time, if any. */
  static async resume(): Promise<Session | null> {
    const resumed = await turn(async () => {
      const login = cachedLogin();
      if (!login) return null;
      const copy = crypto.randomUUID();
      const saved = await load(login.user.id, copy);
      return saved ? new Session(login.user, login.id, copy, saved) : null;
    });
    if (resumed) return resumed;
    try {
      return await Session.open(await api<User>("GET", "/me"));
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) return null;
      throw e;
    }
  }

  /** Asks the server whether the session still holds, and refreshes the user's details. */
  async check(): Promise<boolean> {
    try {
      const me = await api<User>("GET", "/me");
      // Someone else signed in meanwhile, in another tab: this copy is not theirs.
      if (me.id !== this.user.id) return false;
      this.setUser(me);
      return true;
    } catch (e) {
      // Offline, or the server is down: carry on with the local copy.
      return !(e instanceof ApiError && e.status === 401);
    }
  }

  /** Ends the session here, and in the other tabs in the same sign-in, and forgets the user's data
   * on this device. When the device has signed in as the same user again since, here or in another
   * tab, the data is that sign-in's now: only this session's copy goes. */
  close(): Promise<void> {
    return (this.closing ??= this.end());
  }

  private async end() {
    tabs.removeEventListener("message", this.heard);
    this.socket.stop();
    await this.saver.close();
    await turn(async () => {
      const latest = cachedLogin();
      const current = latest?.id === this.login;
      if (current) localStorage.removeItem(LOGIN);
      else if (latest?.user.id === this.user.id) return forgetCopy(this.user.id, this.copy);
      await forget(this.user.id);
      if (current) tabs.postMessage({ ended: this.login } satisfies Message);
    });
  }

  async signOut() {
    await api("POST", "/auth/logout").catch((e) => {
      if (!(e instanceof ApiError && (e.code === OFFLINE || e.status === 401))) throw e;
    });
    await this.close();
  }
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
