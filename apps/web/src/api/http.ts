// Requests to the server. The web app is served by the server and signs in with an HttpOnly cookie,
// so it handles no token. The Android, iOS and desktop apps talk to the server they were told about and
// sign in with a token, both kept in Capacitor Preferences (`localStorage` outside Capacitor), where
// the native side reads them too. See docs/android.md, docs/ios.md and docs/desktop.md.

import { Capacitor } from "@capacitor/core";
import { Preferences } from "@capacitor/preferences";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import type { ErrorBody } from "./types/ErrorBody";

/** The app this runs in, if any. */
export const app = Capacitor.isNativePlatform() ? (Capacitor.getPlatform() as "android" | "ios") : "__TAURI_INTERNALS__" in globalThis ? "desktop" : null;

/** Running in the Android or iOS app, whose native side is the same plugin (mobile.ts). */
export const phoneApp = app === "android" || app === "ios";

/** Running in an app rather than a browser. */
export const native = app !== null;

/** The system the desktop app runs on. */
export const desktopOs = /Windows/.test(navigator.userAgent) ? "Windows" : "macOS";

/** In the app, where the server is and the session's token; empty in the browser. `local`: the
 * app's own server on this device (docs/local.md), which nothing about is kept. */
const server = { url: "", token: undefined as string | undefined, local: false };

/** Reads where the app connects to. Call before anything talks to the server. */
export async function loadServer() {
  if (!native) return;
  server.url = (await Preferences.get({ key: "server" })).value ?? "";
  server.token = (await Preferences.get({ key: "token" })).value ?? undefined;
}

export const serverUrl = () => server.url;

/** Points the app at a server, as typed on the sign-in page: `todo.example.com` means https, an
 * address on the local network plain HTTP. */
export function setServer(typed: string) {
  const url = typed.trim().replace(/\/+$/, "");
  const scheme = /^https?(?=:\/\/)/i.exec(url)?.[0];
  // In lower case, which the sync connection's address is made from.
  server.url = scheme ? scheme.toLowerCase() + url.slice(scheme.length) : `${onLocalNetwork(url) ? "http" : "https"}://${url}`;
  // A token left from an earlier sign-in is the old server's, and never goes to another.
  server.token = undefined;
  server.local = false;
}

/** Whether an address typed without a scheme is on this device or the local network, where servers
 * often have no certificate (docs/design.md#直接用-http). HTTPS is not tried first: a server on the
 * internet would then be reached in the clear whenever someone blocks its HTTPS. */
function onLocalNetwork(typed: string) {
  let host: string;
  try {
    // A name may end in the dot of the DNS root.
    host = new URL(`http://${typed}`).hostname.replace(/\.$/, "");
  } catch {
    // Not an address: the request to it says so.
    return false;
  }
  const v4 = /^(\d+)\.(\d+)\.\d+\.\d+$/.exec(host);
  if (v4) {
    const [a, b] = [Number(v4[1]), Number(v4[2])];
    // Loopback, private, link-local, and the shared range Tailscale gives its devices.
    return a === 127 || a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254) || (a === 100 && b >= 64 && b <= 127);
  }
  if (host.startsWith("[")) {
    // Loopback, unique local (fc00::/7) and link-local (fe80::/10), by the first group in full.
    const first = parseInt(host.slice(1).split(":")[0] || "0", 16);
    return host === "[::1]" || (first & 0xfe00) === 0xfc00 || (first & 0xffc0) === 0xfe80;
  }
  return !host.includes(".") || /\.(local|lan|internal|home\.arpa)$/.test(host);
}

/** Points the app at its own server on this device, for as long as the page is loaded. */
export function connectLocal(url: string, token: string) {
  Object.assign(server, { url, token, local: true });
}

export const connectedLocally = () => server.local;

/** Settles once the app's own server is known to be at its address still (local.ts): what goes to it,
 * the requests and the sync connection, waits for that. */
let atAddress = Promise.resolve();

/** Holds what goes to the app's own server until `checked` settles, in place of any check before. */
export function holdLocal(checked: Promise<void>) {
  atAddress = checked;
}

/** Settles once the server may be sent to: once the latest check has, not one that a later took the
 * place of while it was waited for. Rejects should `signal` abort first: the check can wait behind a
 * backup in the app, which takes long when it downloads files. */
export async function reachable(signal?: AbortSignal | null) {
  while (server.local) {
    const checked = atAddress;
    await (signal ? Promise.race([checked, aborted(signal)]) : checked);
    if (checked === atAddress) return;
  }
}

/** Rejects once `signal` aborts. */
const aborted = (signal: AbortSignal) =>
  new Promise<never>((_, reject) => {
    if (signal.aborted) reject(signal.reason);
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
  });

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

declare global {
  interface Window {
    /** The WebView's own `fetch`, which Capacitor keeps before it sends `fetch` through CapacitorHttp. */
    CapacitorWebFetch: typeof fetch;
  }
}

/** For the app's own server: straight to it. The HTTP plugin goes through the system's proxy, or
 * the one the environment gives, even to 127.0.0.1, and the proxy would get the token and the data.
 * A proxy given here replaces both, and `noProxy` sends 127.0.0.1 past it; `*` would match domain
 * names only. */
const DIRECT = { proxy: { all: { url: "http://127.0.0.1:9", noProxy: "127.0.0.1" } }, maxRedirections: 0 };

/** How long a request may take, its answer included: some are made while the other tabs wait to
 * sign in or out. Those that wait for a model give their own. */
const TIMEOUT_MS = 20_000;

/** Fetches `path` from the server. From another origin than the server's: the phone apps' fetch goes
 * through CapacitorHttp, the desktop app's through Tauri's HTTP plugin, so that the server needs no
 * CORS. To the phone apps' own server, the WebView's: CapacitorHttp would go through the system's
 * proxy. */
export async function fetchServer(path: string, init?: RequestInit): Promise<Response> {
  await reachable(init?.signal);
  const send = app === "desktop" ? tauriFetch : server.local ? window.CapacitorWebFetch : fetch;
  return send(`${server.url}${path}`, init);
}

/** How long sending or fetching a file may take: 20 MB over a slow line. */
const FILE_TIMEOUT_MS = 10 * 60_000;

/** How long an export may take: an account's files can be many. */
const EXPORT_TIMEOUT_MS = 60 * 60_000;

/** Sends or fetches an attachment's file (docs/web.md#附件): the server's answer. What is put is `blob`,
 * or a copy of attachment `from`'s file, which the server makes. */
export async function fileRequest(method: "GET" | "PUT", id: string, stop: AbortSignal, body?: { blob: Blob; type: string } | { from: string }): Promise<Blob> {
  const path = `/api/v1/files/${id}${body && "from" in body ? `?from=${body.from}` : ""}`;
  return (await bulk(method, path, stop, FILE_TIMEOUT_MS, body && "blob" in body ? body : undefined)).answer;
}

/** The user's own data as a backup (docs/local/backup.md#导出), as of `today` here: the file, and the name
 * the server gives it. */
export async function fetchExport(today: string): Promise<{ name: string; file: Blob }> {
  const { answer, headers } = await bulk("GET", `/api/v1/export?today=${today}`, new AbortController().signal, EXPORT_TIMEOUT_MS);
  const name = headers.get("Content-Disposition")?.split("filename*=UTF-8''")[1];
  if (!name) throw new Error("服务器回复缺少文件名");
  return { name: decodeURIComponent(name), file: answer };
}

/** Sends or fetches a file at `path`, the whole of it within `timeoutMs`: the server's answer. The
 * phone apps' goes by the WebView's own fetch, which the server allows across origins: CapacitorHttp
 * would pass the whole file over the bridge. Throws `ApiError`: the server's refusal, or `OFFLINE` when
 * it cannot be reached, the transfer breaks off or takes too long, or `stop` aborts it. */
async function bulk(method: "GET" | "PUT", path: string, stop: AbortSignal, timeoutMs: number, body?: { blob: Blob; type: string }): Promise<{ answer: Blob; headers: Headers }> {
  const headers: Record<string, string> = body ? { "Content-Type": body.type } : {};
  if (server.token) headers.Authorization = `Bearer ${server.token}`;
  const send = app === "desktop" ? tauriFetch : phoneApp ? window.CapacitorWebFetch : fetch;
  const abort = new AbortController();
  const stopped = () => abort.abort();
  stop.addEventListener("abort", stopped);
  if (stop.aborted) stopped();
  const timer = setTimeout(stopped, timeoutMs);
  let res: Response;
  let answer: Blob;
  try {
    const init = { method, headers, body: body?.blob, signal: abort.signal, ...(app === "desktop" && server.local && DIRECT) };
    await reachable(abort.signal);
    res = await send(`${server.url}${path}`, init);
    answer = await res.blob();
  } catch {
    throw new ApiError(0, OFFLINE, server.local ? "无法连接 App 内置的服务端，请重试" : "无法连接服务器，请检查网络后重试");
  } finally {
    clearTimeout(timer);
    stop.removeEventListener("abort", stopped);
  }
  if (res.ok) return { answer, headers: res.headers };
  const error = parseError(await answer.text());
  throw new ApiError(res.status, error.code ?? "error", error.message ?? `服务器错误（${res.status}），请稍后重试`);
}

export async function api<T>(method: string, path: string, body?: unknown, timeoutMs = TIMEOUT_MS): Promise<T> {
  const headers: Record<string, string> = body === undefined ? {} : { "Content-Type": "application/json" };
  if (server.token) headers.Authorization = `Bearer ${server.token}`;
  // Not `AbortSignal.timeout`, which Chromium has from 103 only (docs/web.md#浏览器).
  const timeout = new AbortController();
  const init = {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: timeout.signal,
    ...(app === "desktop" && server.local && DIRECT),
  };
  const timer = setTimeout(() => timeout.abort(), timeoutMs);
  let res: Response;
  let text: string;
  // Only the request itself: a mistake in the code is not the network failing.
  try {
    res = await fetchServer(`/api/v1${path}`, init);
    text = await res.text();
  } catch {
    // The app's own server is on this device: the network has nothing to do with it.
    throw new ApiError(0, OFFLINE, server.local ? "无法连接 App 内置的服务端，请重试" : "无法连接服务器，请检查网络后重试");
  } finally {
    // Answered, the request is not aborted: the desktop app's HTTP plugin would cancel it again.
    clearTimeout(timer);
  }
  // 202 and 204 come without a body.
  if (res.ok) return (text ? JSON.parse(text) : undefined) as T;
  const error = parseError(text);
  const field = (error.details as { field?: string } | undefined)?.field;
  throw new ApiError(res.status, error.code ?? "error", error.message ?? `服务器错误（${res.status}），请稍后重试`, field);
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
  if (app) return `NextTodo App · ${app === "android" ? "Android" : app === "ios" ? "iOS" : desktopOs}`;
  const ua = navigator.userAgent;
  const browser = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "浏览器";
  const os = /iPhone|iPad/.test(ua) ? "iOS" : /Android/.test(ua) ? "Android" : /Mac OS/.test(ua) ? "macOS" : /Windows/.test(ua) ? "Windows" : /Linux/.test(ua) ? "Linux" : "";
  return os ? `${browser} · ${os}` : browser;
}
