// Where the user is. The selected task is `?t=<id>` on top of the page's address, so that the back
// button (Android's too) closes its details.

import { useEffect, useSyncExternalStore } from "react";
import { useLocation, useSearchParams } from "wouter";
import { useHistoryState } from "wouter/use-browser-location";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { inPanel, openMain } from "../desktop";
import { useView } from "../session";

/** A section of the settings, a page of its own (views/Settings.tsx). */
export type SettingsSection = "account" | "local" | "log" | "lock" | "appearance" | "lists" | "reminders" | "widget" | "system" | "ai" | "agent" | "server" | "about";

export const settingsUrl = (section?: SettingsSection) => (section ? `/settings/${section}` : "/settings");

export function useSelection(): { selected: TaskId | null; select(id: TaskId): void; close(): void } {
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const selected = params.get("t");
  return {
    selected,
    select(id) {
      // The panel has no room for details: the main window shows them, on the page the panel shows.
      if (inPanel) {
        const page = location.startsWith("/calendar/") || location === "/agenda" ? "/calendar" : location;
        return void openMain(`${page}?open=${id}`);
      }
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

/** The history entry's state while it asks the page to show a task, “在文件夹中显示”
 * (docs/ui.md#交互细节): in the entry, so that a page slow to load still gets it and it goes with the
 * page; a new one each time it is asked, the same task again too. */
export function useRevealRequest() {
  const state = useHistoryState<{ reveal?: TaskId } | null>();
  return state?.reveal ? state : null;
}

const shown = () => history.replaceState({ ...history.state, reveal: undefined }, "");

/** The task to show in `folder`'s list or graph, and the call to say it has been. Moved away or
 * deleted meanwhile, on another device say, there is none to show. */
export function useShowing(folder: FolderId | null): [Task | null, () => void] {
  const { board } = useView();
  const id = useRevealRequest()?.reveal ?? null;
  const t = id === null ? undefined : board.task(id);
  const gone = id !== null && t?.folder_id !== folder;
  useEffect(() => {
    if (gone) shown();
  }, [gone]);
  return [gone ? null : (t ?? null), shown];
}

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
