// The apps' updates, from the server they are signed in to (docs/design.md#app-更新): on Android,
// newer builds of the web app downloaded for the next start and newer APKs offered to install; on
// macOS, newer versions installed for the next start.

import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { app, serverUrl } from "./api/http";
import { NextTodo } from "./android";

/** What waits for the user: an APK to install, or a version installed that a restart runs. */
export type Pending = { kind: "apk"; version: string; url: string } | { kind: "restart"; version: string } | null;

let pending: Pending = null;
const listeners = new Set<() => void>();

export const usePending = () =>
  useSyncExternalStore(
    (f) => {
      listeners.add(f);
      return () => listeners.delete(f);
    },
    () => pending,
  );

/** The app's version, beside the web app's `import.meta.env.WEB_VERSION`; null in a browser. */
export const appVersion = async () => (app === "android" ? (await NextTodo.appInfo()).version : app === "macos" ? getVersion() : null);

/** How often to look again: the web app changes with nearly every release, the macOS app is a
 * download of a few MB. */
export const EVERY_MS = app === "android" ? 3600_000 : 6 * 3600_000;
/** The server the checks below are of: another one starts afresh. */
let server = "";
let checked = 0;
/** The server being checked, if one is. */
let checking: string | null = null;

/** Looks for updates, unless it did lately. One that fails, offline say, is tried again the next time.
 * With a macOS version installed already, it would only install that again: the app still runs the
 * one before. */
export async function check() {
  if (!app) return;
  if (server !== serverUrl()) {
    server = serverUrl();
    checked = 0;
    if (pending?.kind === "apk") set(null);
  }
  if (checking === server || Date.now() - checked < EVERY_MS || pending?.kind === "restart") return;
  const from = (checking = server);
  // Each on its own: a web build that fails to download does not keep back an APK that fixes it.
  const jobs: Promise<unknown>[] = app === "android" ? [NextTodo.updateWeb(), checkApk(from)] : [checkMac(from)];
  const failed = (await Promise.allSettled(jobs)).filter((r) => r.status === "rejected");
  if (checking === from) checking = null;
  // On another server since, which this check is not of: that one gets its own.
  if (from !== server) return check();
  for (const f of failed) console.warn("检查更新失败", f.reason);
  if (!failed.length) checked = Date.now();
}

/** Offers the APK `from` gives when its native version is newer; nothing, should the app have moved
 * to another server meanwhile. */
async function checkApk(from: string) {
  const res = await fetch(`${from}/app/android.json`);
  if (!res.ok) throw new Error(`/app/android.json：${res.status}`);
  const apk: { native: number; versionName: string; url: string } = await res.json();
  const newer = apk.native > (await NextTodo.appInfo()).native;
  if (from !== serverUrl()) return;
  set(newer ? { kind: "apk", version: apk.versionName, url: new URL(apk.url, `${from}/`).href } : null);
}

/** Installs the macOS version `from` gives when it is newer. */
async function checkMac(from: string) {
  const version = await invoke<string | null>("update", { server: from });
  if (version) set({ kind: "restart", version });
}

/** Tells the listeners about a new update, or none any more; the same found again is not news. */
function set(p: Pending) {
  if (p?.kind === pending?.kind && p?.version === pending?.version) return;
  pending = p;
  for (const f of listeners) f();
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

/** Starts the macOS app again, to run the version installed. */
export const restart = () => invoke("restart");
