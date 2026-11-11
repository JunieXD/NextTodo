// Whether the server signed in to runs the latest release, for its admin to upgrade it
// (docs/design.md#部署): its version, from the app updates it serves, beside the latest on GitHub;
// and upgrading it from the settings.

import { useSyncExternalStore } from "react";
import { api, app, fetchServer, serverUrl } from "./api/http";
import type { UpgradeStatus } from "./api/types/UpgradeStatus";
import { check } from "./updates";

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

/** The version the server runs, from the apps' updates it serves. Given up on after a while: a
 * server restarting can leave it unanswered. */
async function serverVersion(): Promise<string> {
  // Not `AbortSignal.timeout`, and cleared once answered, as `api` does.
  const timeout = new AbortController();
  const timer = setTimeout(() => timeout.abort(), 20_000);
  try {
    return (await json<{ version: string }>(fetchServer("/app/desktop.json", { signal: timeout.signal }))).version;
  } finally {
    clearTimeout(timer);
  }
}

/** Asks for both versions, unless it did lately and it is not `now`. A failure, offline say, is tried
 * again next time; it bothers no one: the row in the settings says the latest is not known. */
export async function checkServer(now = false) {
  const of = serverUrl();
  if (!now && versions?.of === of && Date.now() - checked < SERVER_EVERY_MS) return;
  const [server, latest] = await Promise.allSettled([
    serverVersion(),
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

/** Upgrading from the settings, as the server tells it, and after the switch as far as this side
 * can tell: `restarting` while the server does not answer, since `since`; `upgraded` once the new
 * version does, when the browser loads the new web app after a moment to read it, and the apps ask
 * for their updates at once; `lost` when none has for too long. */
export type Upgrading = { of: string } & (
  | UpgradeStatus
  | { state: "restarting"; version: string; since: number }
  | { state: "upgraded"; version: string }
  | { state: "lost"; version: string }
);

let upgrading: Upgrading | null = null;

export const useUpgrading = () => useSyncExternalStore(subscribe, () => (upgrading?.of === serverUrl() ? upgrading : null));

/** How long the server may take to come back after a switch: it starts in seconds, going back to the
 * version before too. */
const RESTART_MS = 3 * 60_000;
const POLL_MS = 1000;

const ongoing = (u: Upgrading | null) => !!u && ["downloading", "switching", "restarting"].includes(u.state);

function setUpgrading(u: Upgrading) {
  upgrading = u;
  for (const f of listeners) f();
}

/** Asks the server how upgrading goes, and follows it while it goes on. */
export async function followUpgrade() {
  const of = serverUrl();
  const status = await api<UpgradeStatus>("GET", "/admin/upgrade");
  // A switch seen here ends as seen, with the new version or the failure it tells.
  if (serverUrl() !== of || (upgrading?.of === of && ongoing(upgrading))) return;
  setUpgrading({ of, ...status });
  void poll(of);
}

/** Upgrades the server to `version`. */
export async function upgrade(version: string) {
  const of = serverUrl();
  const status = await api<UpgradeStatus>("POST", "/admin/upgrade", { version });
  if (serverUrl() !== of) return;
  setUpgrading({ of, ...status });
  void poll(of);
}

/** The server followed: one at a time for each. */
let polling: string | null = null;

async function poll(of: string) {
  if (polling === of) return;
  polling = of;
  // Checked after each wait: what comes after another server was signed in to is no longer of use.
  const ours = () => serverUrl() === of && upgrading?.of === of;
  try {
    while (ours() && ongoing(upgrading)) {
      await new Promise((r) => setTimeout(r, POLL_MS));
      const before = upgrading;
      if (!ours() || !before || (before.state !== "downloading" && before.state !== "switching" && before.state !== "restarting")) break;
      try {
        const status = await api<UpgradeStatus>("GET", "/admin/upgrade");
        // Idle is a new process: the new version, or the old one again, the switch stopped short. It
        // may come before this side saw the switch, fast as it is.
        const server = status.state === "idle" ? await serverVersion() : null;
        if (!ours()) break;
        if (server === before.version) {
          if (versions?.of === of) versions = { ...versions, server };
          setUpgrading({ of, state: "upgraded", version: before.version });
          if (app) await check(true);
          else setTimeout(() => location.reload(), 2000);
        } else if (server !== null) {
          setUpgrading({ of, state: "failed", version: before.version, message: `升级到 ${before.version} 失败：没能换上新版本，服务器仍是 ${server}，原因见服务器的日志（docker compose logs）` });
        } else setUpgrading({ of, ...status });
      } catch {
        if (!ours()) break;
        // Not answering: on the way to the new version, the switch unseen too, until it is too long.
        // Should it answer still downloading, the network was gone a moment.
        if (before.state !== "restarting") setUpgrading({ of, state: "restarting", version: before.version, since: Date.now() });
        else if (Date.now() - before.since > RESTART_MS) setUpgrading({ of, state: "lost", version: before.version });
      }
    }
  } finally {
    if (polling === of) polling = null;
  }
}
