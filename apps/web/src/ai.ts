// AI organizing on this device: the settings kept here when the user keeps them off the server, and
// handing them over for the jobs waiting. See docs/ai.md.

import { Preferences } from "@capacitor/preferences";
import { useEffect, useSyncExternalStore } from "react";
import { ApiError, OFFLINE, api } from "./api/http";
import type { AiConfig } from "./api/types/AiConfig";
import type { AiSettings } from "./api/types/AiSettings";
import type { Store } from "./sync/store";

const PREFIX = "ai:";
/** Tells the runner of this tab that the settings here changed; other tabs hear `storage`. */
const CHANGED = "nexttodo:ai-config";

/** The settings kept on this device for the user. */
export function localAiConfig(userId: string): AiConfig | null {
  const kept = localStorage.getItem(PREFIX + userId);
  if (!kept) return null;
  const c: AiConfig = JSON.parse(kept);
  // ECNU is no longer a preset; as on the server (migration 003), it goes on as a custom provider.
  return c.provider === "ecnu" ? { ...c, provider: "custom" } : c;
}

export function keepAiConfig(userId: string, c: AiConfig | null) {
  if (c) localStorage.setItem(PREFIX + userId, JSON.stringify(c));
  else localStorage.removeItem(PREFIX + userId);
  dispatchEvent(new Event(CHANGED));
}

export const forgetAiConfig = (userId: string) => localStorage.removeItem(PREFIX + userId);

export function forgetOtherAiConfigs(userId: string) {
  const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!);
  for (const key of keys) if (key.startsWith(PREFIX) && key !== PREFIX + userId) localStorage.removeItem(key);
}

const AI_MODE = "aiMode";
let aiModeOn = false;
const aiModeListeners = new Set<() => void>();

const changeAiMode = (on: boolean) => {
  aiModeOn = on;
  for (const listener of aiModeListeners) listener();
};

/** Reads whether AI organizing was the mode last used to add, which the Android app's add popup shares
 * (docs/android.md#添加弹窗): before anything adds, and again when the app comes back, the popup having
 * maybe switched it meanwhile. */
export async function loadAiMode() {
  changeAiMode((await Preferences.get({ key: AI_MODE })).value === "on");
}

/** The mode last used, as it changes. */
export const useAiMode = () =>
  useSyncExternalStore(
    (listener) => {
      aiModeListeners.add(listener);
      return () => aiModeListeners.delete(listener);
    },
    () => aiModeOn,
  );

export async function setAiMode(on: boolean) {
  changeAiMode(on);
  await Preferences.set({ key: AI_MODE, value: on ? "on" : "off" });
}

/** Whether a model is set up, here or on the server; `true` too when the server cannot be asked,
 * offline say: the job then waits. */
export async function aiReady(userId: string): Promise<boolean> {
  if (localAiConfig(userId)) return true;
  try {
    return (await api<AiSettings | null>("GET", "/ai/settings")) !== null;
  } catch (e) {
    if (!(e instanceof ApiError)) throw e;
    return e.code === OFFLINE;
  }
}

/** How long to wait before asking again after the server could not take a job, at first. */
const RETRY_MS = 5_000;

/** With settings kept on this device, hands them to the server for the oldest job waiting, one at a
 * time: when the data or the settings change, such as a job submitted or one ending. The server
 * turns down a job it has not heard of yet, one already taken, and one while another is being
 * organized, which may be one deleted meanwhile and so seen nowhere: what changed meanwhile has it
 * look again, and otherwise it does after a while, waiting longer each time. Offline, the same. */
export function useLocalRunner(store: Store, userId: string) {
  useEffect(() => {
    let asking = false;
    let again = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let wait = RETRY_MS;
    // Once the session or the user has changed, a request still under way schedules nothing more.
    let stopped = false;
    const look = () => {
      if (stopped) return;
      if (asking) {
        again = true;
        return;
      }
      const config = localAiConfig(userId);
      const jobs = store.getView().board.snapshot.ai_jobs;
      const next = jobs.find((j) => j.state === "waiting");
      if (!config || !next || jobs.some((j) => j.state === "running")) return;
      asking = true;
      again = false;
      api("POST", `/ai/jobs/${next.id}/run`, config)
        .then(
          () => (wait = RETRY_MS),
          (e) => {
            if (!(e instanceof ApiError)) throw e;
            if (stopped || (e.status !== 409 && e.code !== OFFLINE)) return;
            clearTimeout(retry);
            retry = setTimeout(look, wait);
            wait = Math.min(wait * 2, 120_000);
          },
        )
        .finally(() => {
          asking = false;
          if (again) look();
        });
    };
    const stored = (e: StorageEvent) => e.key === PREFIX + userId && look();
    look();
    const unsubscribe = store.subscribe(look);
    addEventListener(CHANGED, look);
    addEventListener("storage", stored);
    return () => {
      stopped = true;
      clearTimeout(retry);
      unsubscribe();
      removeEventListener(CHANGED, look);
      removeEventListener("storage", stored);
    };
  }, [store, userId]);
}
