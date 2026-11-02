// Rows in lists: tasks, folders, and the folder view's tasks with the dependency rail on the left.

import { ChevronRight, Clock, Folder as FolderIcon, Check as CheckIcon, Trash2 } from "lucide-react";
import { type ReactNode, useEffect, useRef } from "react";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { dueKind, formatDue } from "../domain/dates";
import { Check } from "../ui/controls";
import { Icon, Inbound, Outbound } from "../ui/icons";
import { useMarks } from "./marks";

export interface RowActions {
  open(t: Task): void;
  toggle(t: Task): void;
  remove(t: Task): void;
}

/** Where a folder is, as people read it: `学习 / 毕业论文`, or `根目录`. */
export const place = (board: Board, f: FolderId | null) => (f ? board.folderPath(f).slice(1).replaceAll("/", " / ") : "根目录");

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
  railX,
  selected,
  actions,
}: {
  task: Task;
  board: Board;
  now: Date;
  /** Show the folder, in lists that mix folders. */
  path?: boolean;
  /** Where the check sits on the rail. */
  railX?: number;
  selected: boolean;
  actions: RowActions;
}) {
  const marks = useMarks();
  const status = board.status(t);
  const unlocks = status === "ready" ? board.unlocks(t).length : 0;
  const meta: ReactNode[] = [];
  if (path && t.folder_id) meta.push(<span key="path" className="m-path">{place(board, t.folder_id)}</span>);
  if (t.due) {
    meta.push(
      <span key="due" className={dueClass(t, now)}>
        <Icon of={Clock} size={14} />
        {formatDue(t.due, now)}
      </span>,
    );
  }
  if (status === "waiting") {
    meta.push(
      <span key="wait">
        <Icon of={Inbound} size={14} />等 {board.waitingOn(t)} 项
      </span>,
    );
  }
  if (unlocks) {
    meta.push(
      <span key="unlock" className="m-unlock">
        <Icon of={Outbound} size={14} />
        解锁 {unlocks} 项
      </span>,
    );
  }
  const inner = useRef<HTMLDivElement>(null);
  const swiped = useSwipe(inner, (dx) => (dx > 0 ? actions.toggle(t) : actions.remove(t)));
  const cls = ["row", "task", status, marks.flash.has(t.id) && "unlocked", selected && "selected"].filter(Boolean).join(" ");
  return (
    <div className={cls} onClick={() => !swiped.current && actions.open(t)}>
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
        <Check status={status} pop={marks.pop === t.id} left={railX === undefined ? undefined : railX - 12} onToggle={() => actions.toggle(t)} />
        <div className="task-main">
          <div className="task-title">{t.title}</div>
          {meta.length > 0 && <div className="task-meta">{meta}</div>}
        </div>
      </div>
    </div>
  );
}

/** Right swipe past 90px to complete, left to delete. Returns whether a swipe just ended, which is
 * then not also a tap. */
function useSwipe(inner: React.RefObject<HTMLDivElement | null>, done: (dx: number) => void) {
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
      if (!s) return;
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
      if (Math.abs(ended.dx) > 90) latest.current(ended.dx);
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
    <button className="row folder-row" onClick={onOpen}>
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
  );
}

const RAIL_ROW = 64;
const LANE = 24;
const laneX = (c: number) => 28 + c * LANE;

type Edge = [TaskId, TaskId];

/** Leaves out A → C when A → B → C is there too. */
function reduce(edges: Edge[]): Edge[] {
  const next = new Map<TaskId, TaskId[]>();
  for (const [a, b] of edges) next.set(a, [...(next.get(a) ?? []), b]);
  const reaches = (from: TaskId, to: TaskId) => {
    const seen = new Set([from]);
    const stack = [from];
    while (stack.length) {
      const n = stack.pop()!;
      if (n === to) return true;
      for (const m of next.get(n) ?? []) {
        if (!seen.has(m)) {
          seen.add(m);
          stack.push(m);
        }
      }
    }
    return false;
  };
  return edges.filter(([a, b]) => !(next.get(a) ?? []).some((y) => y !== b && reaches(y, b)));
}

/** Lanes like a git commit graph: each dependency runs in a lane from its first task's row to the
 * second's, and parallel branches take separate lanes. */
export function railLayout(list: Task[], board: Board) {
  const index = new Map(list.map((t, i) => [t.id, i]));
  const edges = reduce(list.flatMap((t) => board.after(t).filter((a) => index.has(a.id)).map((a): Edge => [t.id, a.id])));
  const lanes: ({ to: TaskId } | null)[] = [];
  const col = new Map<TaskId, number>();
  const edgeLane = new Map<string, number>();
  const free = () => {
    const i = lanes.findIndex((l) => !l);
    return i < 0 ? lanes.length : i;
  };
  let width = 1;
  for (const t of list) {
    const incoming = lanes.flatMap((l, i) => (l && l.to === t.id ? [i] : []));
    const c = incoming.length ? Math.min(...incoming) : free();
    col.set(t.id, c);
    for (const i of incoming) lanes[i] = null;
    edges
      .filter(([a]) => a === t.id)
      .sort((x, y) => index.get(x[1])! - index.get(y[1])!)
      .forEach(([, to], k) => {
        const l = k === 0 && !lanes[c] ? c : free();
        lanes[l] = { to };
        edgeLane.set(`${t.id}>${to}`, l);
      });
    width = Math.max(width, lanes.length, c + 1);
  }
  const gutter = 16 + width * LANE + 12;
  const paths = edges.map(([a, b]) => {
    const [xu, xl, xv] = [laneX(col.get(a)!), laneX(edgeLane.get(`${a}>${b}`)!), laneX(col.get(b)!)];
    const [yu, yv] = [index.get(a)! * RAIL_ROW + 32, index.get(b)! * RAIL_ROW + 32];
    let d = `M${xu} ${yu}`;
    if (xl !== xu) d += ` L${xu} ${yu + 12} C${xu} ${yu + 24} ${xl} ${yu + 20} ${xl} ${yu + 32}`;
    d += ` L${xl} ${yv - 32}`;
    if (xl !== xv) d += ` C${xl} ${yv - 20} ${xv} ${yv - 24} ${xv} ${yv - 12}`;
    d += ` L${xv} ${yv}`;
    return { key: `${a}>${b}`, d, live: board.status(board.task(a)!) === "ready" };
  });
  return { gutter, height: list.length * RAIL_ROW, paths, x: (t: Task) => laneX(col.get(t.id)!) };
}

export function RailList({ tasks, board, now, selected, actions }: { tasks: Task[]; board: Board; now: Date; selected: TaskId | null; actions: RowActions }) {
  const rail = railLayout(tasks, board);
  return (
    <div className="rail-list" style={{ "--gutter": `${rail.gutter}px`, "--sep-left": `${rail.gutter}px` } as React.CSSProperties}>
      <svg className="rail" width={rail.gutter} height={rail.height}>
        {rail.paths.map((p) => (
          <path key={p.key} d={p.d} className={p.live ? "live" : undefined} />
        ))}
      </svg>
      {tasks.map((t) => (
        <TaskRow key={t.id} task={t} board={board} now={now} railX={rail.x(t)} selected={selected === t.id} actions={actions} />
      ))}
    </div>
  );
}
