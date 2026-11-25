// AI organizing on this device: the settings kept here when the user keeps them off the server, and
// handing them over for the jobs waiting. See docs/ai.md.

import { Preferences } from "@capacitor/preferences";
import { useCallback, useEffect, useEffectEvent, useState, useSyncExternalStore } from "react";
import { ApiError, OFFLINE, api } from "./api/http";
import type { AiConfig } from "./api/types/AiConfig";
import type { AiJobId } from "./api/types/AiJobId";
import type { AiSettings } from "./api/types/AiSettings";
import type { Store } from "./sync/store";

const PREFIX = "ai:";
/** Tells the runner of this tab that the settings here changed; other tabs hear `storage`. */
const CHANGED = "nexttodo:ai-config";

/** The settings kept on this device for the user. */
export const localAiConfig = (userId: string) => parsed(localStorage.getItem(PREFIX + userId));

function parsed(kept: string | null): AiConfig | null {
  if (!kept) return null;
  // Kept before the model could be shown pictures: it is not.
  const c: AiConfig = { vision: false, ...JSON.parse(kept) };
  // ECNU is no longer a preset; as on the server (migration 003), it goes on as a custom provider.
  return c.provider === "ecnu" ? { ...c, provider: "custom" } : c;
}

export function keepAiConfig(userId: string, c: AiConfig | null) {
  if (c) localStorage.setItem(PREFIX + userId, JSON.stringify(c));
  else localStorage.removeItem(PREFIX + userId);
  dispatchEvent(new Event(CHANGED));
}

/** Calls `f` when the settings kept here for the user change, in this window or another. */
function onConfig(userId: string, f: () => void): () => void {
  const stored = (e: StorageEvent) => e.key === PREFIX + userId && f();
  addEventListener(CHANGED, f);
  addEventListener("storage", stored);
  return () => {
    removeEventListener(CHANGED, f);
    removeEventListener("storage", stored);
  };
}

const SERVER = "ai-server:";

/** The model of the user's settings on the server as last heard, for the settings row to show at
 * once: null for none, undefined before it was ever heard. */
export function serverAiModel(userId: string): string | null | undefined {
  const kept = localStorage.getItem(SERVER + userId);
  return kept === null ? undefined : JSON.parse(kept);
}

export const keepServerAiModel = (userId: string, model: string | null) => localStorage.setItem(SERVER + userId, JSON.stringify(model));

export function forgetAiConfig(userId: string) {
  localStorage.removeItem(PREFIX + userId);
  localStorage.removeItem(SERVER + userId);
}

export function forgetOtherAiConfigs(userId: string) {
  const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!);
  for (const key of keys) {
    if ((key.startsWith(PREFIX) && key !== PREFIX + userId) || (key.startsWith(SERVER) && key !== SERVER + userId)) localStorage.removeItem(key);
  }
}

const AI_MODE = "aiMode";
let aiModeOn = false;
const aiModeListeners = new Set<() => void>();

const changeAiMode = (on: boolean) => {
  aiModeOn = on;
  for (const listener of aiModeListeners) listener();
};

/** Reads whether AI organizing was the mode last used to add: before anything adds. */
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

/** Whether the server has a model set up for the user, as it says now, which is kept as last heard;
 * unknown when it cannot be asked, offline say. Any other failure counts as none. */
async function onServer(userId: string): Promise<boolean | undefined> {
  try {
    const settings = await api<AiSettings | null>("GET", "/ai/settings");
    keepServerAiModel(userId, settings?.model ?? null);
    return settings !== null;
  } catch (e) {
    if (!(e instanceof ApiError)) throw e;
    return e.code === OFFLINE ? undefined : false;
  }
}

/** Whether a model is set up, here or on the server; also when the server cannot be asked, offline
 * say: the job then waits. */
export const aiReady = async (userId: string) => localAiConfig(userId) !== null || ((await onServer(userId)) ?? true);

/** Whether a model is known to be set up, here or on the server, offline as last heard: what another
 * app shares goes to AI organizing only then, not on a guess (docs/android.md#快速添加小窗). */
export const aiSetUp = async (userId: string) => localAiConfig(userId) !== null || ((await onServer(userId)) ?? !!serverAiModel(userId));

/** Whether the model in use sees pictures (docs/ai.md#附件), as the settings kept here say, or else
 * those on the server, asked each time `asking` turns true (callers make that wait for being online);
 * unknown until it answers, and when it cannot. */
export function useAiVision(userId: string, asking: boolean): boolean | undefined {
  const watch = useCallback((changed: () => void) => onConfig(userId, changed), [userId]);
  const local = parsed(useSyncExternalStore(watch, () => localStorage.getItem(PREFIX + userId)));
  const here = local !== null;
  const [server, setServer] = useState<boolean>();
  useEffect(() => {
    if (!asking || here) return;
    let live = true;
    api<AiSettings | null>("GET", "/ai/settings").then(
      (s) => live && setServer(s?.vision),
      (e) => {
        if (!(e instanceof ApiError)) throw e;
      },
    );
    return () => {
      live = false;
    };
  }, [asking, here]);
  return local ? local.vision : server;
}

/** How long to wait before asking again after the server could not take a job, at first. */
const RETRY_MS = 5_000;

/** With settings kept on this device, hands them to the server for the oldest job waiting, one at a
 * time: when the data or the settings change, such as a job submitted or one ending. The server
 * turns down a job it has not heard of yet, one already taken, and one while another is being
 * organized, which may be one deleted meanwhile and so seen nowhere: what changed meanwhile has it
 * look again, and otherwise it does after a while, waiting longer each time. Offline, the same; and
 * when the drive it syncs through has not taken the start in time (docs/local.md#排定先后). A job it
 * turns down for any other reason, such as the settings, `failed` hears of, and it waits until they
 * change. */
export function useLocalRunner(store: Store, userId: string, failed: (job: AiJobId, e: ApiError) => void) {
  const fail = useEffectEvent(failed);
  useEffect(() => {
    let asking = false;
    let again = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let wait = RETRY_MS;
    // Once the session or the user has changed, a request still under way schedules nothing more.
    let stopped = false;
    const refused = new Set<AiJobId>();
    /** Counts the changes of the settings: a job turned down with ones before is not set aside. */
    let settings = 0;
    const look = () => {
      if (stopped) return;
      if (asking) {
        again = true;
        return;
      }
      const config = localAiConfig(userId);
      const jobs = store.getView().board.snapshot.ai_jobs;
      const next = jobs.find((j) => j.state === "waiting" && !refused.has(j.id));
      if (!config || !next || jobs.some((j) => j.state === "running")) return;
      asking = true;
      again = false;
      const asked = settings;
      api("POST", `/ai/jobs/${next.id}/run`, config)
        .then(
          () => (wait = RETRY_MS),
          (e) => {
            if (!(e instanceof ApiError)) throw e;
            if (stopped) return;
            if (e.status !== 409 && e.status !== 503 && e.code !== OFFLINE) {
              again = true;
              if (asked !== settings) return;
              refused.add(next.id);
              fail(next.id, e);
              return;
            }
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
    look();
    const unsubscribe = store.subscribe(look);
    const unwatch = onConfig(userId, () => {
      settings++;
      refused.clear();
      look();
    });
    return () => {
      stopped = true;
      clearTimeout(retry);
      unsubscribe();
      unwatch();
    };
  }, [store, userId]);
}
