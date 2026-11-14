// Batch operations (docs/ui.md#批量操作): tasks chosen on a folder's page or on what is done, then
// completed, reopened, moved or deleted together, as one change that one undo takes back.

import { Check as CheckIcon, FolderInput, type LucideIcon, RotateCcw, Trash2, X } from "lucide-react";
import { type Dispatch, type ReactNode, type SetStateAction, createContext, useContext, useEffect, useLayoutEffect, useState } from "react";
import { useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { lastKeys } from "../domain/apply";
import { isoDate } from "../domain/dates";
import { useCommit, useSession, useUndo, useView } from "../session";
import { useBack } from "../ui/back";
import { Tip, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { useMarking } from "./marks";
import { useFolderView, useSelection } from "./nav";
import { FolderPicker } from "./pickers";

const them = (n: number) => (n === 1 ? "它" : "它们");

/** What one change may hold, as `MAX_OPS` in crates/core/src/ops.rs has it. */
const MAX_OPS = 1000;

/** A page batch operations are on: a folder's (`null` for the root), or what is done. */
type Place = FolderId | null | "done";

function placeAt(location: string): Place {
  if (location === "/done") return "done";
  return location.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
}

interface Chosen {
  /** The page's address: leaving it ends the batch. */
  at: string;
  ids: ReadonlySet<TaskId>;
  /** The task tapped last, from which Shift + click chooses those in between. */
  anchor: TaskId | null;
}

interface BatchState {
  chosen: Chosen | null;
  setChosen: Dispatch<SetStateAction<Chosen | null>>;
  /** The tasks the page lists while choosing, in order, as the page says. */
  listed: TaskId[];
  setListed: Dispatch<SetStateAction<TaskId[]>>;
  /** Starts choosing on this page, with `first` chosen. */
  start(first?: TaskId): void;
}

const BatchContext = createContext<BatchState>(null!);

export function BatchProvider({ children }: { children: ReactNode }) {
  const [location] = useLocation();
  const [view, setView] = useFolderView();
  const { selected, close } = useSelection();
  const [chosen, setChosen] = useState<Chosen | null>(null);
  const [listed, setListed] = useState<TaskId[]>([]);
  if (chosen && chosen.at !== location) setChosen(null);
  useBack("panel", chosen ? () => setChosen(null) : null);
  const start = (first?: TaskId) => {
    // Chosen in the list, with no task open.
    if (view === "graph") setView("list");
    if (selected) close();
    setChosen({ at: location, ids: new Set(first ? [first] : []), anchor: first ?? null });
  };
  return <BatchContext value={{ chosen, setChosen, listed, setListed, start }}>{children}</BatchContext>;
}

/** Says which tasks a page lists, in order, for choosing them all or a run of them. */
export function useListed(tasks: Task[]) {
  const { chosen, setListed } = useContext(BatchContext);
  const on = chosen !== null;
  useLayoutEffect(() => {
    if (!on) return;
    const ids = tasks.map((t) => t.id);
    setListed((was) => (was.length === ids.length && was.every((id, i) => id === ids[i]) ? was : ids));
  });
}

/** What a row needs: whether tasks are being chosen, whether it is, and choosing it. */
export function useChoice() {
  const { chosen, setChosen, listed, start } = useContext(BatchContext);
  return {
    on: chosen !== null,
    has: (id: TaskId) => chosen?.ids.has(id) ?? false,
    start,
    /** Chooses `id` or lets it go; with `run`, chooses every task from the one tapped last. */
    tap(id: TaskId, run: boolean) {
      setChosen((c) => {
        if (!c) return c;
        const ids = new Set(c.ids);
        const [from, to] = [listed.indexOf(c.anchor!), listed.indexOf(id)];
        if (run && from >= 0 && to >= 0) for (const x of listed.slice(Math.min(from, to), Math.max(from, to) + 1)) ids.add(x);
        else if (!ids.delete(id)) ids.add(id);
        return { ...c, ids, anchor: id };
      });
    },
  };
}

/** The batch as the bars show it: the tasks chosen, as listed, and choosing all or none. */
export function useBatch() {
  const { chosen, setChosen, listed, start } = useContext(BatchContext);
  const { board } = useView();
  const tasks = chosen ? listed.filter((id) => chosen.ids.has(id)).flatMap((id) => board.task(id) ?? []) : [];
  const all = listed.length > 0 && tasks.length === listed.length;
  return {
    on: chosen !== null,
    place: chosen && placeAt(chosen.at),
    tasks,
    all,
    start,
    end: () => setChosen(null),
    toggleAll: () => setChosen((c) => c && { ...c, ids: new Set(all ? [] : listed), anchor: null }),
  };
}

interface BatchAction {
  icon: LucideIcon;
  label: string;
  danger?: boolean;
  run(): void;
}

/** What can be done to the tasks chosen on `place`, each asked first; and the panel to pick a folder
 * in. Done, the batch ends. */
function useBatchActions(place: Place, chosen: Task[], end: () => void): { actions: BatchAction[]; panel: ReactNode } {
  const { store } = useSession();
  const commit = useCommit();
  const undo = useUndo();
  const { confirm, snack } = useFeedback();
  const marking = useMarking();
  const [moving, setMoving] = useState(false);
  const board = () => store.getView().board;
  const fits = (n: number) => n <= MAX_OPS || (snack(`每次最多处理 ${MAX_OPS} 项`), false);
  /** `tasks` as they are now that the dialog is answered, those still on the page: another device may
   * have changed them meanwhile. */
  const still = (tasks: Task[]) => tasks.flatMap((t) => board().task(t.id) ?? []).filter((t) => (place === "done" ? t.done_at !== null : t.folder_id === place));
  /** Says what was done, with the undo, and ends the batch. */
  const done = (r: { id: MutationId }, text: string) => {
    snack({ text, action: "撤销", onAction: () => undo(r.id) });
    end();
  };

  const complete = async () => {
    const todo = chosen.filter((t) => t.done_at === null);
    if (!fits(todo.length)) return;
    const waiting = todo.filter((t) => board().status(t) === "waiting").length;
    const repeating = todo.filter((t) => t.due?.repeat).length;
    const notes = [waiting && `其中 ${waiting} 项还在等待`, repeating && `重复的 ${repeating} 项将顺延到下一次`].filter(Boolean);
    const body = notes.length ? `${notes.join("，")}。` : "可撤销。";
    if (!(await confirm({ title: `将 ${todo.length} 项标为完成？`, body, ok: "标为完成" }))) return;
    const going = still(todo).filter((t) => t.done_at === null);
    if (!going.length) return end();
    const readyBefore = new Set(board().next().map((t) => t.id));
    // A repeating task's next due date counts from the day here.
    const today = isoDate(new Date());
    const r = commit(going.map((t): Op => ({ op: "task.update", id: t.id, done: true, ...(t.due?.repeat && { today }) })));
    if (!r) return;
    const unlocked = board()
      .next()
      .filter((t) => !readyBefore.has(t.id))
      .map((t) => t.id);
    // As one completed alone: each stays a moment before folding away, and what they unlocked flashes.
    going.filter((t) => !t.due?.repeat).forEach((t, i) => marking.completed(t.id, i ? [] : unlocked));
    done(r, unlocked.length ? `已完成 ${going.length} 项，解锁了 ${unlocked.length} 项` : `已完成 ${going.length} 项`);
  };

  const reopen = async () => {
    const todo = chosen.filter((t) => t.done_at !== null);
    if (!fits(todo.length)) return;
    if (!(await confirm({ title: `重新打开 ${todo.length} 项？`, body: `${them(todo.length)}的后续可能因此变回等待中。`, ok: "重新打开" }))) return;
    const going = still(todo).filter((t) => t.done_at !== null);
    if (!going.length) return end();
    const r = commit(going.map((t): Op => ({ op: "task.update", id: t.id, done: false })));
    if (r) done(r, `已恢复 ${going.length} 项`);
  };

  const move = (folder: FolderId | null) => {
    if (!chosen.length || !fits(chosen.length)) return;
    // Last there, in the order they were listed.
    const keys = lastKeys(
      board()
        .contents(folder)
        .map((t) => t.sort_key),
      chosen.length,
    );
    const r = commit(chosen.map((t, i): Op => ({ op: "task.update", id: t.id, folder_id: folder, sort_key: keys[i] })));
    if (r) done(r, `已将 ${chosen.length} 项移至“${board().folderName(folder)}”`);
  };

  const remove = async () => {
    if (!fits(chosen.length)) return;
    const body = `连着${them(chosen.length)}的依赖一并删除，可撤销。`;
    if (!(await confirm({ title: `删除 ${chosen.length} 项？`, body, ok: "删除", danger: true }))) return;
    const going = still(chosen);
    if (!going.length) return end();
    const r = commit(going.map((t): Op => ({ op: "task.delete", id: t.id })));
    if (r) done(r, `已删除 ${going.length} 项`);
  };

  const reopening: BatchAction = { icon: RotateCcw, label: "重新打开", run: reopen };
  const deleting: BatchAction = { icon: Trash2, label: "删除", danger: true, run: remove };
  if (place === "done") return { actions: [reopening, deleting], panel: null };
  const allDone = chosen.length > 0 && chosen.every((t) => t.done_at !== null);
  return {
    actions: [allDone ? reopening : { icon: CheckIcon, label: "完成", run: complete }, { icon: FolderInput, label: "移动到", run: () => setMoving(true) }, deleting],
    panel: moving && <FolderPicker mode="move" current={place} onPick={move} onClose={() => setMoving(false)} />,
  };
}

/** The title while choosing. */
export const batchTitle = (n: number) => (n ? `已选 ${n} 项` : "选择事项");

/** The phone's top bar while choosing: leave, how many are chosen, all or none. */
export function BatchHead() {
  const batch = useBatch();
  return (
    <>
      <Tip label="退出批量操作">
        <button className="icon-btn" onClick={batch.end}>
          <Icon of={X} />
        </button>
      </Tip>
      <span className="batch-title">{batchTitle(batch.tasks.length)}</span>
      <button className="text-btn batch-all" onClick={batch.toggleAll}>
        {batch.all ? "全不选" : "全选"}
      </button>
    </>
  );
}

/** The phone's bottom bar while choosing, in place of the tabs and quick add: what can be done. */
export function BatchBar() {
  const batch = useBatch();
  const { actions, panel } = useBatchActions(batch.place!, batch.tasks, batch.end);
  return (
    <>
      <nav className="tabbar batch-bar" style={{ gridTemplateColumns: `repeat(${actions.length}, 1fr)` }}>
        {actions.map((a) => (
          <button key={a.label} className={`tab${a.danger ? " danger" : ""}`} disabled={!batch.tasks.length} onClick={a.run}>
            <span className="tab-ic">
              <Icon of={a.icon} size={22} />
            </span>
            {a.label}
          </button>
        ))}
      </nav>
      {panel}
    </>
  );
}

/** The middle column's head on a computer while choosing: all or none, what can be done, leave; and
 * the keys for them. */
export function BatchTools() {
  const batch = useBatch();
  const { actions, panel } = useBatchActions(batch.place!, batch.tasks, batch.end);
  const remove = actions.find((a) => a.danger)!.run;
  const { toggleAll, end, all, tasks } = batch;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      // Not what a field, a dialog or a menu takes, nor a drag cut short.
      if (e.defaultPrevented || (e.target as Element).closest("input, textarea, [role=dialog], [role=alertdialog], [role=menu]")) return;
      if (e.key === "Escape") end();
      else if ((e.key === "Delete" || e.key === "Backspace") && tasks.length) remove();
      else if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
        if (!all) toggleAll();
      }
      else return;
      e.preventDefault();
    };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  });
  return (
    <div className="head-actions">
      <button className="text-btn" onClick={toggleAll}>
        {all ? "全不选" : "全选"}
      </button>
      {actions.map((a) => (
        <button key={a.label} className={`tonal-btn${a.danger ? " danger-text" : ""}`} disabled={!tasks.length} onClick={a.run}>
          <Icon of={a.icon} size={18} />
          {a.label}
        </button>
      ))}
      <button className="text-btn" onClick={end}>
        退出
      </button>
      {panel}
    </div>
  );
}
