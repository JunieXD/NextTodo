// Rows in lists: tasks, folders, and the folder view's tasks with the dependency rail on the left.

import { CalendarClock, CheckCheck, ChevronRight, Clock, Folder as FolderIcon, FolderInput, Check as CheckIcon, PanelRightOpen, Repeat, RotateCcw, Trash2 } from "lucide-react";
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FolderId } from "../api/types/FolderId";
import type { Op } from "../api/types/Op";
import type { Folder } from "../api/types/Folder";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { dueKind, formatDue, isoDate, startLabel } from "../domain/dates";
import { plainRail, railLayout } from "../domain/rail";
import { range, rekey } from "../domain/reorder";
import { useCommit } from "../session";
import { useFeedback } from "../ui/feedback";
import { useWide } from "../ui/hooks";
import { Box, Check } from "../ui/controls";
import { Icon, Inbound, Outbound } from "../ui/icons";
import { ContextMenu, type MenuItem } from "../ui/menu";
import { useChoice } from "./batch";
import { useRowDrag } from "./drag";
import { FolderContextMenu } from "./FolderMenu";
import { useFolderView } from "./nav";
import { useMarks } from "./marks";
import { FolderPicker } from "./pickers";

export interface RowActions {
  open(t: Task): void;
  toggle(t: Task): void;
  /** Stops a repeating task so that it is done. */
  endRepeat(t: Task): void;
  move(t: Task, folder: FolderId | null): void;
  remove(t: Task): void;
}

/** `overdue` or `today` for a pending task due then, which is when the date is coloured. */
export function dueClass(t: Task, now: Date): string {
  if (t.done_at !== null || !t.due) return "";
  const kind = dueKind(t.due, now);
  return kind === "overdue" || kind === "today" ? kind : "";
}

export function TaskRow({
  task: t,
  board,
  now,
  path,
  step,
  railX,
  open,
  batchable,
  selected,
  actions,
}: {
  task: Task;
  board: Board;
  now: Date;
  /** Show the folder, in lists that mix folders. */
  path?: boolean;
  /** Show which step it is, in “接下来”, which has no rail to show it (docs/design.md#关联进度). */
  step?: boolean;
  /** Where the check sits on the rail. */
  railX?: number;
  /** In a list of open tasks, which a completed one leaves by folding away. */
  open?: boolean;
  /** On a page where tasks can be chosen for [batch operations](docs/ui.md#批量操作). */
  batchable?: boolean;
  selected: boolean;
  actions: RowActions;
}) {
  const marks = useMarks();
  const status = board.status(t);
  const unlocks = status === "ready" ? board.unlocks(t).length : 0;
  const meta: ReactNode[] = [];
  if (path && t.folder_id) meta.push(<span key="path" className="m-path">{board.place(t.folder_id)}</span>);
  if (t.done_at === null && t.completions?.last === isoDate(now)) {
    meta.push(
      <span key="today" className="m-done-today">
        <Icon of={CheckIcon} size={14} />
        今天已完成
      </span>,
    );
  }
  if (t.due) {
    meta.push(
      <span key="due" className={dueClass(t, now)}>
        <Icon of={t.due.repeat ? Repeat : Clock} size={14} />
        {formatDue(t.due, now)}
      </span>,
    );
  }
  if (status === "waiting" && board.waitingOn(t) > 0) {
    meta.push(
      <span key="wait">
        <Icon of={Inbound} size={14} />等 {board.waitingOn(t)} 项
      </span>,
    );
  }
  if (t.done_at === null && board.notStarted(t)) {
    meta.push(
      <span key="start">
        <Icon of={CalendarClock} size={14} />
        {startLabel(t.start!, now)}
      </span>,
    );
  }
  const at = step ? board.step(t) : undefined;
  if (unlocks) {
    meta.push(
      <span key="unlock" className="m-unlock">
        <Icon of={Outbound} size={14} />
        解锁 {unlocks} 项
      </span>,
    );
  }
  const row = useRef<HTMLDivElement>(null);
  const leaving = open && marks.leaving.has(t.id) && t.done_at !== null;
  // Folding from the height it has.
  useLayoutEffect(() => {
    if (leaving) row.current!.style.setProperty("--h", `${row.current!.offsetHeight}px`);
  }, [leaving]);
  const choice = useChoice();
  const choosing = !!batchable && choice.on;
  const chosen = choosing && choice.has(t.id);
  const inner = useRef<HTMLDivElement>(null);
  const swiped = useSwipe(inner, choosing ? null : (dx) => (dx > 0 ? actions.toggle(t) : actions.remove(t)));
  const [moving, setMoving] = useState(false);
  const menu: MenuItem[] = [
    ...(selected ? [] : [{ icon: PanelRightOpen, label: "打开", onSelect: () => actions.open(t) }]),
    t.done_at === null ? { icon: CheckIcon, label: "标为完成", onSelect: () => actions.toggle(t) } : { icon: RotateCcw, label: "重新打开", onSelect: () => actions.toggle(t) },
    ...(t.due?.repeat ? [{ icon: CheckCheck, label: "结束重复并完成", onSelect: () => actions.endRepeat(t) }] : []),
    { icon: FolderInput, label: "移动到…", onSelect: () => setMoving(true) },
    { icon: Trash2, label: "删除", danger: true, onSelect: () => actions.remove(t) },
  ];
  const cls = ["row", "task", status, marks.flash.has(t.id) && "unlocked", leaving && "leaving", selected && "selected", chosen && "chosen"].filter(Boolean).join(" ");
  const tap = (e: React.MouseEvent) => {
    if (swiped.current) return;
    if (choosing) choice.tap(t.id, e.shiftKey);
    // Ctrl, or ⌘ on a Mac, where Control with a click is a right click.
    else if (batchable && (e.ctrlKey || e.metaKey)) choice.start(t.id);
    else actions.open(t);
  };
  // The picker is not inside the row, where its clicks would also open the task.
  return (
    <>
      <ContextMenu items={menu} off={choosing}>
        <div ref={row} className={cls} onClick={tap}>
          <div className="swipe-bg">
            <span>
              <Icon of={CheckIcon} size={18} />
              完成
            </span>
            <span>
              删除
              <Icon of={Trash2} size={18} />
            </span>
          </div>
          <div className="row-inner" ref={inner}>
            <Check
              status={status}
              pop={marks.pop === t.id}
              left={railX === undefined ? undefined : railX - 12}
              onToggle={() => (choosing ? choice.tap(t.id, false) : actions.toggle(t))}
            />
            {/* The whole row opens the task; this button is how keyboards and screen readers get there. */}
            <button type="button" className="task-main">
              <span className="task-head">
                <span className="task-title">{t.title}</span>
                {at && (
                  <span className="task-step">
                    {at[0]}/{at[1]}
                  </span>
                )}
              </span>
              {meta.length > 0 && <span className="task-meta">{meta}</span>}
            </button>
            {choosing && <Box checked={chosen} />}
          </div>
        </div>
      </ContextMenu>
      {moving && <FolderPicker mode="move" current={t.folder_id} onPick={(f: FolderId | null) => actions.move(t, f)} onClose={() => setMoving(false)} />}
    </>
  );
}

/** Right swipe past 90px to complete, left to delete; none without `done`. Returns whether a swipe
 * just ended, which is then not also a tap. */
function useSwipe(inner: React.RefObject<HTMLDivElement | null>, done: ((dx: number) => void) | null) {
  const swiped = useRef(false);
  const latest = useRef(done);
  useEffect(() => {
    latest.current = done;
  });
  useEffect(() => {
    const el = inner.current!;
    const row = el.parentElement!;
    let s: { x: number; y: number; dx: number; active: boolean } | null = null;
    const start = (e: TouchEvent) => {
      if (e.touches.length === 1) s = { x: e.touches[0].clientX, y: e.touches[0].clientY, dx: 0, active: false };
    };
    const move = (e: TouchEvent) => {
      // A row picked up to be dragged elsewhere doesn't swipe.
      if (!s || !latest.current || "lifted" in row.dataset) return;
      const dx = e.touches[0].clientX - s.x;
      if (!s.active) {
        if (Math.abs(e.touches[0].clientY - s.y) > 10) {
          s = null;
          return;
        }
        if (Math.abs(dx) < 12) return;
        s.active = true;
        row.classList.add("swiping");
      }
      s.dx = dx;
      el.style.transform = `translateX(${dx}px)`;
    };
    const end = () => {
      const ended = s;
      s = null;
      if (!ended?.active) return;
      swiped.current = true;
      setTimeout(() => (swiped.current = false), 50);
      row.classList.remove("swiping");
      el.style.transform = "";
      if (Math.abs(ended.dx) > 90) latest.current?.(ended.dx);
    };
    row.addEventListener("touchstart", start, { passive: true });
    row.addEventListener("touchmove", move, { passive: true });
    row.addEventListener("touchend", end);
    return () => {
      row.removeEventListener("touchstart", start);
      row.removeEventListener("touchmove", move);
      row.removeEventListener("touchend", end);
    };
  }, [inner]);
  return swiped;
}

export function FolderRow({ folder, board, onOpen }: { folder: Folder; board: Board; onOpen(): void }) {
  const own = board.contents(folder.id).filter((t) => t.done_at === null);
  const ready = own.filter((t) => board.status(t) === "ready").length;
  const sub = own.length ? `${ready} 项可做，共 ${own.length} 项待办` : `${board.subfolders(folder.id).length} 个文件夹`;
  return (
    <FolderContextMenu folder={folder.id}>
      <button className="row folder-row" data-into={folder.id} onClick={onOpen}>
        <div className="row-inner">
          <span className="folder-ic">
            <Icon of={FolderIcon} size={18} />
          </span>
          <span className="folder-main">
            <span>{folder.name}</span>
            <span className="folder-sub">{sub}</span>
          </span>
          <span className="chev">
            <Icon of={ChevronRight} size={18} />
          </span>
        </div>
      </button>
    </FolderContextMenu>
  );
}

/** How many lanes the rail may take before the list goes without it: on a phone, on a computer. */
const MAX_LANES = { narrow: 3, wide: 5 };

export function RailList({ tasks, board, now, selected, actions }: { tasks: Task[]; board: Board; now: Date; selected: TaskId | null; actions: RowActions }) {
  // With more lanes the titles get squeezed: the graph view shows them better.
  const layout = railLayout(tasks, board, MAX_LANES[useWide() ? "wide" : "narrow"]);
  const rail = layout ?? { ...plainRail, x: () => plainRail.x };
  const [, setView] = useFolderView();
  const commit = useCommit();
  const { snack } = useFeedback();
  const list = useRef<HTMLDivElement>(null);
  const choice = useChoice();
  useRowDrag(list, ".row.task", {
    off: choice.on,
    range: (from) => range(board, tasks, from),
    drop(from, to) {
      const t = tasks[from];
      const { min, max, above, below } = range(board, tasks, from);
      if (to < min) snack(`“${t.title}”需等待“${above!.title}”完成，只能排在其后`);
      else if (to > max) snack(`“${below!.title}”需等待“${t.title}”完成，“${t.title}”只能排在其前`);
      else if (to !== from) {
        const wanted = tasks.filter((x) => x !== t);
        wanted.splice(to, 0, t);
        commit(rekey(wanted, t.id).map((k): Op => ({ op: "task.update", ...k })));
      }
    },
    canInto: (from, folder) => folder !== tasks[from].folder_id,
    into: (from, folder) => actions.move(tasks[from], folder),
  });
  return (
    <>
      {!layout && (
        <button className="row rail-note" onClick={() => setView("graph")}>
          <div className="row-inner">
            依赖较多，列表里不画连线
            <span className="text-primary">看关系图</span>
          </div>
        </button>
      )}
      <div className="rail-list reorderable" ref={list} style={{ "--gutter": `${rail.gutter}px`, "--sep-left": `${rail.gutter}px` } as React.CSSProperties}>
        {layout && (
          <svg className="rail" width={layout.gutter} height={layout.height}>
            {layout.paths.map((p) => (
              <path key={p.key} d={p.d} className={p.live ? "live" : undefined} />
            ))}
          </svg>
        )}
        {tasks.map((t) => (
          <TaskRow key={t.id} task={t} board={board} now={now} railX={rail.x(t)} open batchable selected={selected === t.id} actions={actions} />
        ))}
      </div>
    </>
  );
}
