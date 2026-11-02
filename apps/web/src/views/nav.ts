// Where the user is. The selected task is `?t=<id>` on top of the page's address, so that the back
// button (Android's too) closes its details.

import { useLocation, useSearchParams } from "wouter";
import type { TaskId } from "../api/types/TaskId";

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
