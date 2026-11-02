// The WebSocket to the server. See `docs/api.md#同步协议` and `docs/web.md#同步`.

import type { ClientMessage } from "../api/types/ClientMessage";
import type { Mutation } from "../api/types/Mutation";
import type { ServerMessage } from "../api/types/ServerMessage";
import type { Store } from "./store";

export type Connection = "connecting" | "online" | "offline";

export interface SocketOptions {
  /** `ws://` or `wss://` URL of `/api/v1/sync`. */
  url: string;
  /** For apps that sign in with a token; the web app's cookie goes along by itself. */
  token?: string;
  /** Whose local copy this is: the server refuses the connection if someone else is signed in. */
  user: string;
  /** Downloads everything again; called when the server says the local copy is too old. */
  resync(): Promise<void>;
  /** The server no longer accepts the session. */
  signedOut(): void;
  status(c: Connection): void;
}

const FIRST_RETRY = 1000;
const LAST_RETRY = 30_000;

export class Socket {
  private ws: WebSocket | null = null;
  private synced = false;
  private retry = FIRST_RETRY;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private stopped = false;
  /** Downloading a fresh copy after `resync`; the connection waits for it. */
  private resyncing = false;

  constructor(
    private readonly store: Store,
    private readonly opts: SocketOptions,
  ) {}

  start() {
    addEventListener("online", this.wake);
    document.addEventListener("visibilitychange", this.wake);
    this.connect();
  }

  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
    removeEventListener("online", this.wake);
    document.removeEventListener("visibilitychange", this.wake);
    this.ws?.close();
  }

  /** Sends a change now if connected; otherwise it goes out with the rest after reconnecting. */
  send(m: Mutation) {
    if (this.synced) this.write({ type: "mutate", mutation: m });
  }

  /** Back in front, or back online: reconnect now rather than at the next retry. */
  private wake = () => {
    if (document.visibilityState === "visible" && !this.ws && !this.resyncing && !this.stopped) {
      clearTimeout(this.timer);
      this.connect();
    }
  };

  private connect() {
    this.opts.status("connecting");
    const ws = new WebSocket(this.opts.url);
    this.ws = ws;
    ws.onopen = () => this.write({ type: "hello", token: this.opts.token, since: this.store.since, user: this.opts.user });
    ws.onmessage = (e) => this.handle(JSON.parse(e.data) as ServerMessage);
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.synced = false;
      this.retryLater();
    };
  }

  private retryLater() {
    if (this.stopped) return;
    this.opts.status("offline");
    this.timer = setTimeout(() => this.connect(), this.retry);
    this.retry = Math.min(this.retry * 2, LAST_RETRY);
  }

  private handle(msg: ServerMessage) {
    switch (msg.type) {
      case "changes":
        this.store.receive(msg.changes, msg.seq);
        if (!this.synced) {
          // Caught up: everything still unconfirmed goes again, in order. The server knows the
          // ids, so a change that did arrive before is not applied twice.
          this.synced = true;
          this.retry = FIRST_RETRY;
          this.opts.status("online");
          for (const m of this.store.unconfirmed) this.write({ type: "mutate", mutation: m });
        }
        return;
      case "ack":
        this.store.acked(msg.id, msg.seq);
        return;
      case "reject":
        this.store.reject(msg.id, msg);
        return;
      case "resync": {
        // The server closes the connection; reconnect from the new copy at once.
        this.ws?.close();
        this.ws = null;
        this.resyncing = true;
        this.opts
          .resync()
          .then(
            () => !this.stopped && this.connect(),
            () => this.retryLater(),
          )
          .finally(() => (this.resyncing = false));
        return;
      }
      case "error":
        if (msg.code === "unauthenticated") {
          this.stop();
          this.opts.signedOut();
        }
        return;
    }
  }

  private write(msg: ClientMessage) {
    this.ws?.send(JSON.stringify(msg));
  }
}
