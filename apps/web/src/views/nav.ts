// Where the user is. The selected task is `?t=<id>` on top of the page's address, so that the back
// button (Android's too) closes its details.

import { useSyncExternalStore } from "react";
import { useLocation, useSearchParams } from "wouter";
import type { TaskId } from "../api/types/TaskId";

/** A section of the settings, a page of its own (views/Settings.tsx). */
export type SettingsSection = "account" | "local" | "appearance" | "background" | "ai" | "agent" | "invites" | "about";

export const settingsUrl = (section?: SettingsSection) => (section ? `/settings/${section}` : "/settings");

export function useSelection(): { selected: TaskId | null; select(id: TaskId): void; close(): void } {
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const selected = params.get("t");
  return {
    selected,
    select(id) {
      // Opening pushes an entry for back to close; switching to another task replaces it.
      navigate(`${location}?t=${id}`, selected ? { replace: true, state: history.state } : { state: { sheet: true } });
    },
    close() {
      if (history.state?.sheet) history.back();
      else navigate(location, { replace: true });
    },
  };
}

export const folderUrl = (id: string | null) => (id ? `/folders/${id}` : "/folders");

export type FolderView = "list" | "graph";

const VIEW_KEY = "nexttodo.folderView";
const viewListeners = new Set<() => void>();

/** Whether folders show as a list or a graph, remembered on this device. */
export function useFolderView(): [FolderView, (v: FolderView) => void] {
  const view = useSyncExternalStore(
    (l) => {
      viewListeners.add(l);
      return () => viewListeners.delete(l);
    },
    () => (localStorage.getItem(VIEW_KEY) as FolderView | null) ?? "list",
  );
  const setView = (v: FolderView) => {
    localStorage.setItem(VIEW_KEY, v);
    for (const l of viewListeners) l();
  };
  return [view, setView];
}
