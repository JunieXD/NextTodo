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

/** Why updates could not be had, as the apps say (`Failure` in apps/desktop/src-tauri/src/updates.rs,
 * `kind` in apps/android): the place they come from out of reach, nothing to be had there for now, or
 * a version downloaded that would not install. */
export type Failure = "offline" | "unavailable" | "install";

/** When updates were last looked for, and why that failed if it did; whether a newer build of the web
 * app has been downloaded, for the next start. */
export type Checked = { at: number; failure: Failure | null; staged: boolean } | null;

let pending: Pending = null;
let lastChecked: Checked = null;
let staged = false;
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
export const useChecking = () => useSyncExternalStore(subscribe, () => checking);

/** In local mode, with no server: the latest release on GitHub, whose files GitHub redirects to. Its
 * manifests name their downloads in full. */
const GITHUB = "https://github.com/JunieXD/NextTodo/releases/latest/download";

/** Where updates come from: the server signed in to, or GitHub. */
const source = () => (inLocalMode() ? GITHUB : serverUrl());

/** The manifest `name` of `from`: the server has them under `/app/`. */
const manifest = (from: string, name: string) => (from === GITHUB ? `${GITHUB}/${name}` : `${from}/app/${name}`);

/** The installed app's version, which on Android the web app's updates outgrow; null in a browser. */
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

/** Looks for updates, unless it did lately and the user did not ask (`now`). One that fails, offline
 * say, is tried again the next time. With a desktop version downloaded already, it would only
 * download that again: the app still runs the one before. One check at a time, of one place: the app
 * has one web build and one version to install at a time, wherever they come from; one asked for
 * meanwhile comes after. */
export async function check(now = false) {
  if (!app) return;
  if (checking) {
    again = true;
    return;
  }
  if (from !== source()) {
    from = source();
    checked = 0;
    staged = false;
    if (pending?.kind === "apk") set(null);
  }
  if ((!now && Date.now() - checked < EVERY_MS) || pending?.kind === "restart") return;
  checking = true;
  told();
  // Each on its own: a web build that fails to download does not keep back an APK that fixes it. In
  // local mode the web app comes with the APK only.
  const web = from === GITHUB ? [] : [NextTodo.updateWeb({ server: from }).then((r) => (staged ||= !!r.staged))];
  const jobs: Promise<unknown>[] = app === "android" ? [...web, checkApk(from)] : [checkDesktop(from)];
  const failed = (await Promise.allSettled(jobs)).filter((r) => r.status === "rejected");
  checking = false;
  for (const f of failed) console.warn("检查更新失败", f.reason);
  if (!failed.length) checked = Date.now();
  lastChecked = { at: Date.now(), failure: failed.length ? failureOf(failed[0].reason) : null, staged };
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
  const res = await fetch(url).catch((e) => Promise.reject(new Failed("offline", `${url}：${e}`)));
  if (!res.ok) throw new Failed("unavailable", `${url}：${res.status}`);
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

/** A failure of the page's own, with what went wrong for the console. */
class Failed extends Error {
  constructor(
    readonly code: Failure,
    message: string,
  ) {
    super(message);
  }
}

/** The kind of failure `e` is: the desktop app rejects with it, Android with it as the code. Others,
 * an answer that is no manifest say, mean nothing usable is there. */
function failureOf(e: unknown): Failure {
  const kind = typeof e === "string" ? e : (e as { code?: unknown } | null)?.code;
  return kind === "offline" || kind === "install" ? kind : "unavailable";
}

/** What the user is told of `f`, which made `doing` fail. */
export function failureText(doing: string, f: Failure) {
  const why = {
    offline: `无法连接${inLocalMode() ? " GitHub" : "服务器"}，请检查网络后重试`,
    unavailable: "暂时无法获取新版本，请稍后重试",
    install: "新版本无法安装，请从官网下载最新版本",
  };
  return `${doing}失败：${why[f]}`;
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
    if (!(await NextTodo.installApp({ url })).started) snack("请允许 NextTodo 安装应用，然后重试");
  } catch (e) {
    console.warn("下载新版本失败", e);
    snack(failureText("下载新版本", failureOf(e)));
  }
}

/** Starts the desktop app again, to run the version downloaded; says so when that fails. */
export const restart = (snack: (s: string) => void) =>
  invoke("restart").catch((e) => (console.warn("重启失败", e), snack(failureText("重启", failureOf(e)))));
