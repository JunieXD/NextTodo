// The apps' updates, from the server they are signed in to (docs/design.md#app-更新), or in local mode
// from GitHub (docs/local.md#更新): on Android, newer builds of the web app downloaded for the next
// start and newer APKs offered to install; on the desktop, newer versions downloaded for the next
// start.

import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { app, serverUrl } from "./api/http";
import { NextTodo } from "./android";
import { inLocalMode } from "./local";

/** What waits for the user: an APK to install, or a version downloaded that a restart runs. */
export type Pending = { kind: "apk"; version: string; url: string } | { kind: "restart"; version: string } | null;

/** When updates were last looked for, and whether that failed. */
export type Checked = { at: number; failed: boolean } | null;

let pending: Pending = null;
let lastChecked: Checked = null;
const listeners = new Set<() => void>();
const subscribe = (f: () => void) => {
  listeners.add(f);
  return () => listeners.delete(f);
};
const told = () => {
  for (const f of listeners) f();
};

export const usePending = () => useSyncExternalStore(subscribe, () => pending);
export const useChecked = () => useSyncExternalStore(subscribe, () => lastChecked);

/** In local mode, with no server: the latest release on GitHub, whose files GitHub redirects to. Its
 * manifests name their downloads in full. */
const GITHUB = "https://github.com/JunieXD/NextTodo/releases/latest/download";

/** Where updates come from: the server signed in to, or GitHub. */
const source = () => (inLocalMode() ? GITHUB : serverUrl());

/** The manifest `name` of `from`: the server has them under `/app/`. */
const manifest = (from: string, name: string) => (from === GITHUB ? `${GITHUB}/${name}` : `${from}/app/${name}`);

/** The app's version, beside the web app's `import.meta.env.WEB_VERSION`; null in a browser. */
export const appVersion = async () => (app === "android" ? (await NextTodo.appInfo()).version : app === "desktop" ? getVersion() : null);

/** How often to look again: the web app changes with nearly every release, the desktop app is a
 * download of a few MB. */
export const EVERY_MS = app === "android" ? 3600_000 : 6 * 3600_000;
/** Where the checks below are of: another place starts afresh. */
let from = "";
let checked = 0;
let checking = false;
/** Whether a check was asked for while one was under way, after signing in to another server say. */
let again = false;

/** Looks for updates, unless it did lately. One that fails, offline say, is tried again the next time.
 * With a desktop version downloaded already, it would only download that again: the app still runs
 * the one before. One check at a time, of one place: the app has one web build and one version to
 * install at a time, wherever they come from; one asked for meanwhile comes after. */
export async function check() {
  if (!app) return;
  if (checking) {
    again = true;
    return;
  }
  if (from !== source()) {
    from = source();
    checked = 0;
    if (pending?.kind === "apk") set(null);
  }
  if (Date.now() - checked < EVERY_MS || pending?.kind === "restart") return;
  checking = true;
  // Each on its own: a web build that fails to download does not keep back an APK that fixes it. In
  // local mode the web app comes with the APK only.
  const web = from === GITHUB ? [] : [NextTodo.updateWeb({ server: from })];
  const jobs: Promise<unknown>[] = app === "android" ? [...web, checkApk(from)] : [checkDesktop(from)];
  const failed = (await Promise.allSettled(jobs)).filter((r) => r.status === "rejected");
  checking = false;
  for (const f of failed) console.warn("检查更新失败", f.reason);
  if (!failed.length) checked = Date.now();
  lastChecked = { at: Date.now(), failed: failed.length > 0 };
  told();
  if (again) {
    again = false;
    return check();
  }
}

/** Offers the APK `of` gives when its native version is newer; nothing, should the app have moved
 * elsewhere meanwhile. A server gives it relative to its address. */
async function checkApk(of: string) {
  const url = manifest(of, "android.json");
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}：${res.status}`);
  const apk: { native: number; versionName: string; url: string } = await res.json();
  const newer = apk.native > (await NextTodo.appInfo()).native;
  if (of !== source()) return;
  set(newer ? { kind: "apk", version: apk.versionName, url: new URL(apk.url, `${of}/`).href } : null);
}

/** Downloads the desktop version `of` gives when it is newer. */
async function checkDesktop(of: string) {
  const version = await invoke<string | null>("update", { manifest: manifest(of, "desktop.json") });
  if (version) set({ kind: "restart", version });
}

/** Tells the listeners about a new update, or none any more; the same found again is not news. */
function set(p: Pending) {
  if (p?.kind === pending?.kind && p?.version === pending?.version) return;
  pending = p;
  told();
}

/** On Android, says the web app has started, so that a newly downloaded build is kept. */
export function started() {
  if (app === "android") void NextTodo.webReady();
}

/** Downloads the APK at `url` and opens the system installer, saying what happens meanwhile. */
export async function install(url: string, snack: (s: string) => void) {
  snack("正在下载新版本…");
  try {
    // The app may not install apps yet: the page that allows it has opened.
    if (!(await NextTodo.installApp({ url })).started) snack("请允许 NextTodo 安装应用，然后再试一次");
  } catch (e) {
    snack(`下载新版本失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

/** Starts the desktop app again, to run the version downloaded; says so when that fails. */
export const restart = (snack: (s: string) => void) => invoke("restart").catch((e) => snack(String(e)));
