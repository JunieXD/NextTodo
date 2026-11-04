// AI organizing on this device: the settings kept here when the user keeps them off the server, and
// handing them over for the jobs waiting. See docs/ai.md.

import { useEffect, useRef } from "react";
import { ApiError, OFFLINE, api } from "./api/http";
import type { AiConfig } from "./api/types/AiConfig";
import type { AiSettings } from "./api/types/AiSettings";
import type { Store } from "./sync/store";

const PREFIX = "ai:";

/** The settings kept on this device for the user. */
export function localAiConfig(userId: string): AiConfig | null {
  const kept = localStorage.getItem(PREFIX + userId);
  return kept && JSON.parse(kept);
}

export function keepAiConfig(userId: string, c: AiConfig | null) {
  if (c) localStorage.setItem(PREFIX + userId, JSON.stringify(c));
  else localStorage.removeItem(PREFIX + userId);
}

export const forgetAiConfig = (userId: string) => localStorage.removeItem(PREFIX + userId);

export function forgetOtherAiConfigs(userId: string) {
  const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!);
  for (const key of keys) if (key.startsWith(PREFIX) && key !== PREFIX + userId) localStorage.removeItem(key);
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

/** With settings kept on this device, hands them to the server for the oldest job waiting, one at a
 * time, whenever the data changes: a job submitted, or one ending. The server turns down a job
 * already taken, or one while another is being organized; the change that ends it tries again. */
export function useLocalRunner(store: Store, userId: string) {
  const asking = useRef(false);
  useEffect(() => {
    const look = () => {
      const config = localAiConfig(userId);
      const jobs = store.getView().board.snapshot.ai_jobs;
      const next = jobs.find((j) => j.state === "waiting");
      if (!config || asking.current || !next || jobs.some((j) => j.state === "running")) return;
      asking.current = true;
      api("POST", `/ai/jobs/${next.id}/run`, config)
        .catch((e) => {
          if (!(e instanceof ApiError)) throw e;
        })
        .finally(() => (asking.current = false));
    };
    look();
    const unsubscribe = store.subscribe(look);
    return () => void unsubscribe();
  }, [store, userId]);
}
