// Requests to the server. The web app is served by the server and signs in with an HttpOnly cookie,
// so it handles no token. The Android app talks to the server it was told about and signs in with a
// token, both kept in Capacitor Preferences, where the widget reads them too. See docs/android.md.

import { Capacitor } from "@capacitor/core";
import { Preferences } from "@capacitor/preferences";
import type { ErrorBody } from "./types/ErrorBody";

/** Running in the Android app rather than a browser. */
export const native = Capacitor.isNativePlatform();

export const DEFAULT_SERVER = "https://todo.juniexd.cn";

/** In the app, where the server is and the session's token; empty in the browser. */
const server = { url: "", token: undefined as string | undefined };

/** Reads where the app connects to. Call before anything talks to the server. */
export async function loadServer() {
  if (!native) return;
  server.url = (await Preferences.get({ key: "server" })).value ?? DEFAULT_SERVER;
  server.token = (await Preferences.get({ key: "token" })).value ?? undefined;
}

export const serverUrl = () => server.url;

/** Points the app at a server, as typed on the sign-in page: `todo.example.com` means https. */
export function setServer(typed: string) {
  const url = typed.trim().replace(/\/+$/, "");
  server.url = /^https?:\/\//.test(url) ? url : `https://${url}`;
  // A token left from an earlier sign-in is the old server's, and never goes to another.
  server.token = undefined;
}

/** Remembers the server and the session after signing in in the app. */
export async function keepSession(token: string) {
  if (!native) return;
  server.token = token;
  await Preferences.set({ key: "server", value: server.url });
  await Preferences.set({ key: "token", value: token });
}

/** Forgets the session's token in the app; the server stays for next time. */
export async function dropSession() {
  if (!native) return;
  server.token = undefined;
  await Preferences.remove({ key: "token" });
}

/** The WebSocket address, and the token to say hello with in the app. */
export const syncUrl = () => `${server.url ? server.url.replace(/^http/, "ws") : `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}`}/api/v1/sync`;
export const syncToken = () => server.token;

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    /** The request field the error is about, when there is one. */
    readonly field?: string,
  ) {
    super(message);
  }
}

export const OFFLINE = "offline";

/** How long a request may take, its answer included: some are made while the other tabs wait to
 * sign in or out. */
const TIMEOUT_MS = 20_000;

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  let res: Response;
  let text: string;
  try {
    const headers: Record<string, string> = body === undefined ? {} : { "Content-Type": "application/json" };
    if (server.token) headers.Authorization = `Bearer ${server.token}`;
    res = await fetch(`${server.url}/api/v1${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    text = await res.text();
  } catch {
    throw new ApiError(0, OFFLINE, "连不上服务器，请检查网络");
  }
  if (res.status === 204) return undefined as T;
  if (res.ok) return JSON.parse(text) as T;
  const error = parseError(text);
  const field = (error.details as { field?: string } | undefined)?.field;
  throw new ApiError(res.status, error.code ?? "error", error.message ?? `服务器出错了（${res.status}）`, field);
}

/** The server's error, or nothing when a proxy in between answers with a page of its own. */
function parseError(text: string): Partial<ErrorBody> {
  try {
    return JSON.parse(text);
  } catch {
    return {};
  }
}

/** How this browser shows up among the user's devices, such as `Chrome · macOS`. */
export function deviceName(): string {
  if (native) return "NextTodo App · Android";
  const ua = navigator.userAgent;
  const browser = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "浏览器";
  const os = /iPhone|iPad/.test(ua) ? "iOS" : /Android/.test(ua) ? "Android" : /Mac OS/.test(ua) ? "macOS" : /Windows/.test(ua) ? "Windows" : /Linux/.test(ua) ? "Linux" : "";
  return os ? `${browser} · ${os}` : browser;
}
