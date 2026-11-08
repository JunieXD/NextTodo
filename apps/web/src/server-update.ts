// Whether the server signed in to runs the latest release, for its admin to upgrade it
// (docs/design.md#部署): its version, from the app updates it serves, beside the latest on GitHub.

import { useSyncExternalStore } from "react";
import { fetchServer, serverUrl } from "./api/http";

/** The latest release, which GitHub's API gives pages from any origin. */
const LATEST = "https://api.github.com/repos/JunieXD/NextTodo/releases/latest";

/** How often to ask again, while the app stays open. */
export const SERVER_EVERY_MS = 24 * 3600_000;

/** The server's version and the latest release's; `latest` null when GitHub could not be asked. `of`:
 * the server they are about, as the app may sign in to another. */
export type ServerVersions = { of: string; server: string; latest: string | null };

let versions: ServerVersions | null = null;
/** When both were last had, of `versions.of`. */
let checked = 0;
const listeners = new Set<() => void>();
const subscribe = (f: () => void) => {
  listeners.add(f);
  return () => listeners.delete(f);
};

export const useServerVersions = () => useSyncExternalStore(subscribe, () => (versions?.of === serverUrl() ? versions : null));

/** Whether version `a` (`0.1.150`) is newer than `b`. */
export function newer(a: string, b: string): boolean {
  const [x, y] = [a, b].map((v) => v.split(".").map(Number));
  const i = x.findIndex((n, i) => n !== y[i]);
  return i >= 0 && x[i] > (y[i] ?? -1);
}

async function json<T>(res: Promise<Response>): Promise<T> {
  const r = await res;
  if (!r.ok) throw new Error(`${r.url}：${r.status}`);
  return r.json();
}

/** Asks for both versions, unless it did lately. A failure, offline say, is tried again next time;
 * it bothers no one: the row in the settings says the latest is not known. */
export async function checkServer() {
  const of = serverUrl();
  if (versions?.of === of && Date.now() - checked < SERVER_EVERY_MS) return;
  const [server, latest] = await Promise.allSettled([
    json<{ version: string }>(fetchServer("/app/desktop.json")).then((m) => m.version),
    json<{ tag_name: string }>(fetch(LATEST)).then((r) => r.tag_name.replace(/^v/, "")),
  ]);
  for (const r of [server, latest]) if (r.status === "rejected") console.warn("查询服务器版本失败", r.reason);
  // A server without the apps' updates, a development one say, has no version to tell; one left
  // meanwhile for another, no use.
  if (server.status === "rejected" || serverUrl() !== of) return;
  checked = latest.status === "fulfilled" ? Date.now() : 0;
  versions = { of, server: server.value, latest: latest.status === "fulfilled" ? latest.value : null };
  for (const f of listeners) f();
}
