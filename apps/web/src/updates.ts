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
const EVERY_MS = app === "android" ? 3600_000 : 6 * 3600_000;
let checked = 0;
let checking = false;

/** Looks for updates, unless it did lately. One that fails, offline say, is tried again the next time.
 * With a macOS version installed already, it would only install that again: the app still runs the
 * one before. */
export async function check() {
  if (!app || checking || Date.now() - checked < EVERY_MS || pending?.kind === "restart") return;
  checking = true;
  try {
    if (app === "android") {
      await NextTodo.updateWeb();
      const res = await fetch(`${serverUrl()}/app/android.json`);
      if (!res.ok) throw new Error(`/app/android.json：${res.status}`);
      const apk: { native: number; versionName: string; url: string } = await res.json();
      if (apk.native > (await NextTodo.appInfo()).native) {
        set({ kind: "apk", version: apk.versionName, url: new URL(apk.url, `${serverUrl()}/`).href });
      }
    } else {
      const version = await invoke<string | null>("update", { server: serverUrl() });
      if (version) set({ kind: "restart", version });
    }
    checked = Date.now();
  } catch (e) {
    console.warn("检查更新失败", e);
  } finally {
    checking = false;
  }
}

/** Tells the listeners about a new update; the same APK found again is not news. */
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
