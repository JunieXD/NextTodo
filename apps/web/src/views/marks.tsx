// Short-lived marks after completing a task: its check pops, it stays in place a moment before
// folding away, and the tasks it unlocked flash.

import { type ReactNode, createContext, useContext, useMemo, useState } from "react";
import type { TaskId } from "../api/types/TaskId";

interface Marks {
  pop: TaskId | null;
  linger: Set<TaskId>;
  /** Lingering, now folding away. */
  leaving: Set<TaskId>;
  flash: Set<TaskId>;
}

interface Marking {
  pop(id: TaskId): void;
  completed(id: TaskId, unlocked: TaskId[]): void;
}

const MarksContext = createContext<Marks>({ pop: null, linger: new Set(), leaving: new Set(), flash: new Set() });
const MarkingContext = createContext<Marking>(null!);

export const useMarks = () => useContext(MarksContext);
export const useMarking = () => useContext(MarkingContext);

const LINGER_MS = 900;
/** As long as `.task.leaving` takes to fold (index.css). */
const LEAVE_MS = 300;
const FLASH_MS = 1700;
const POP_MS = 400;

export function MarksProvider({ children }: { children: ReactNode }) {
  const [marks, setMarks] = useState<Marks>({ pop: null, linger: new Set(), leaving: new Set(), flash: new Set() });
  const marking = useMemo<Marking>(() => {
    const change = (key: "linger" | "leaving" | "flash", ids: TaskId[], add: boolean) =>
      setMarks((m) => {
        const set = new Set(m[key]);
        for (const id of ids) {
          if (add) set.add(id);
          else set.delete(id);
        }
        return { ...m, [key]: set };
      });
    const pop = (id: TaskId) => {
      setMarks((m) => ({ ...m, pop: id }));
      setTimeout(() => setMarks((m) => (m.pop === id ? { ...m, pop: null } : m)), POP_MS);
    };
    return {
      pop,
      completed(id, unlocked) {
        pop(id);
        change("linger", [id], true);
        setTimeout(() => change("leaving", [id], true), LINGER_MS);
        setTimeout(() => (change("linger", [id], false), change("leaving", [id], false)), LINGER_MS + LEAVE_MS);
        if (!unlocked.length) return;
        change("flash", unlocked, true);
        setTimeout(() => change("flash", unlocked, false), FLASH_MS);
      },
    };
  }, []);
  return (
    <MarkingContext value={marking}>
      <MarksContext value={marks}>{children}</MarksContext>
    </MarkingContext>
  );
}
