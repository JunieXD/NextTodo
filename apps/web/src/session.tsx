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
import { forget, load, saver } from "./sync/persist";
import { type Connection, Socket } from "./sync/socket";
import { type Saved, Store } from "./sync/store";
import { useFeedback } from "./ui/feedback";

const USER = "nexttodo.user";

/** What the UI wants to hear about; set once it is on screen. */
export interface Listeners {
  rejected(m: Mutation, e: ErrorBody): void;
  arrived(entries: LogEntry[]): void;
  signedOut(): void;
}

export class Session {
  readonly store: Store;
  private readonly socket: Socket;
  connection: Connection = "connecting";
  private readonly watchers = new Set<() => void>();
  private listeners: Listeners = { rejected() {}, arrived() {}, signedOut() {} };

  constructor(
    public user: User,
    saved: Saved,
  ) {
    const save = saver(user.id);
    this.store = new Store(saved, {
      save,
      send: (m) => this.socket.send(m),
      rejected: (m, e) => this.listeners.rejected(m, e),
      arrived: (entries) => this.listeners.arrived(entries),
    });
    this.socket = new Socket(this.store, {
      url: `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/api/v1/sync`,
      resync: async () => this.store.resync(await api<SeqSnapshot>("GET", "/snapshot")),
      signedOut: () => this.listeners.signedOut(),
      status: (c) => {
        this.connection = c;
        for (const w of this.watchers) w();
      },
    });
    this.socket.start();
  }

  listen(listeners: Listeners) {
    this.listeners = listeners;
  }

  /** The session has ended elsewhere. */
  ended() {
    this.listeners.signedOut();
  }

  setUser(user: User) {
    this.user = user;
    localStorage.setItem(USER, JSON.stringify(user));
  }

  watchConnection = (w: () => void) => {
    this.watchers.add(w);
    return () => this.watchers.delete(w);
  };

  /** Signs in as `user`, from the copy on this device or a fresh one from the server. */
  static async open(user: User): Promise<Session> {
    localStorage.setItem(USER, JSON.stringify(user));
    return new Session(user, (await load(user.id)) ?? (await download()));
  }

  /** The session this device was in last time, if any. */
  static async resume(): Promise<Session | null> {
    const cached = localStorage.getItem(USER);
    if (cached) {
      const user: User = JSON.parse(cached);
      const saved = await load(user.id);
      if (saved) return new Session(user, saved);
    }
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
      this.setUser(await api<User>("GET", "/me"));
      return true;
    } catch (e) {
      // Offline, or the server is down: carry on with the local copy.
      return !(e instanceof ApiError && e.status === 401);
    }
  }

  /** Forgets everything about the user on this device. */
  async close() {
    this.socket.stop();
    localStorage.removeItem(USER);
    await forget(this.user.id);
  }

  async signOut() {
    await api("POST", "/auth/logout").catch((e) => {
      if (!(e instanceof ApiError && (e.code === OFFLINE || e.status === 401))) throw e;
    });
    await this.close();
  }
}

async function download(): Promise<Saved> {
  const { seq, ...base } = await api<SeqSnapshot>("GET", "/snapshot");
  return { ...Store.empty(), seq, base };
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
