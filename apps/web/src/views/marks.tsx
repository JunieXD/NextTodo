// Short-lived marks after completing a task: its check pops, it stays in place a moment before
// folding away, and the tasks it unlocked flash.

import { type ReactNode, createContext, useContext, useMemo, useState } from "react";
import type { TaskId } from "../api/types/TaskId";
import type { Dated } from "../domain/board";

interface Marks {
  pop: TaskId | null;
  /** Each with the deadline of a task after it that it was listed by in “接下来” (`Board.dueBy`):
   * once done it has none, and completing it may have fixed or ended those after it. */
  linger: ReadonlyMap<TaskId, Dated | undefined>;
  /** Lingering, now folding away. */
  leaving: Set<TaskId>;
  flash: Set<TaskId>;
}

interface Marking {
  pop(id: TaskId): void;
  completed(id: TaskId, unlocked: TaskId[], by: Dated | undefined): void;
  /** Tasks just unlocked, by a repeating task moving on or by several completed at once. */
  flash(ids: TaskId[]): void;
}

const MarksContext = createContext<Marks>({ pop: null, linger: new Map(), leaving: new Set(), flash: new Set() });
const MarkingContext = createContext<Marking>(null!);

export const useMarks = () => useContext(MarksContext);
export const useMarking = () => useContext(MarkingContext);

const LINGER_MS = 900;
/** As long as `.task.leaving` takes to fold (index.css). */
const LEAVE_MS = 300;
const FLASH_MS = 1700;
const POP_MS = 400;

export function MarksProvider({ children }: { children: ReactNode }) {
  const [marks, setMarks] = useState<Marks>({ pop: null, linger: new Map(), leaving: new Set(), flash: new Set() });
  const marking = useMemo<Marking>(() => {
    const change = (key: "leaving" | "flash", ids: TaskId[], add: boolean) =>
      setMarks((m) => {
        const set = new Set(m[key]);
        for (const id of ids) {
          if (add) set.add(id);
          else set.delete(id);
        }
        return { ...m, [key]: set };
      });
    const linger = (id: TaskId, add: boolean, by?: Dated) =>
      setMarks((m) => {
        const linger = new Map(m.linger);
        if (add) linger.set(id, by);
        else linger.delete(id);
        return { ...m, linger };
      });
    /** The lingering tasks' timers. */
    const timers = new Map<TaskId, ReturnType<typeof setTimeout>[]>();
    const pop = (id: TaskId) => {
      setMarks((m) => ({ ...m, pop: id }));
      setTimeout(() => setMarks((m) => (m.pop === id ? { ...m, pop: null } : m)), POP_MS);
    };
    const flash = (ids: TaskId[]) => {
      if (!ids.length) return;
      change("flash", ids, true);
      setTimeout(() => change("flash", ids, false), FLASH_MS);
    };
    return {
      pop,
      flash,
      completed(id, unlocked, by) {
        pop(id);
        // Completed again after an undo, it starts over.
        for (const t of timers.get(id) ?? []) clearTimeout(t);
        change("leaving", [id], false);
        linger(id, true, by);
        timers.set(id, [
          setTimeout(() => change("leaving", [id], true), LINGER_MS),
          setTimeout(() => (timers.delete(id), linger(id, false), change("leaving", [id], false)), LINGER_MS + LEAVE_MS),
        ]);
        flash(unlocked);
      },
    };
  }, []);
  return (
    <MarkingContext value={marking}>
      <MarksContext value={marks}>{children}</MarksContext>
    </MarkingContext>
  );
}
