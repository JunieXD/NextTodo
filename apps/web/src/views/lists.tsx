// The bodies of the main pages: what is next, what is done, a folder (the root too), search results.

import { ChevronDown, ChevronRight, FolderPlus, List, ListFilter, Workflow } from "lucide-react";
import { Fragment, lazy, Suspense, useEffect, useRef, useState } from "react";
import { useLocation } from "wouter";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import { matches } from "../domain/board";
import { dueKind, formatDay } from "../domain/dates";
import { useView } from "../session";
import { readyIn, setNextFolders, useNextFolders } from "../next-folders";
import { Fold } from "../ui/fold";
import { Tip } from "../ui/feedback";
import { Seg } from "../ui/controls";
import { Icon } from "../ui/icons";
import { useWide } from "../ui/hooks";
import { useTaskActions } from "./actions";
import { useListed } from "./batch";
import { Chain, useChains } from "./chain";
import { type DueGroup, setGroupClosed, useClosedGroups } from "./closed-groups";
import { moved, useRowDrag } from "./drag";
import { useNewFolder, usePlaceFolder } from "./FolderMenu";
import { useMarking, useMarks } from "./marks";
import { folderUrl, useFolderView, useSelection, useShowing } from "./nav";
import { JobRows } from "./jobs";
import { useKeepDone } from "./keep-done";
import { FolderChecks } from "./pickers";
import { FolderRow, RailList, TaskRow } from "./rows";
import { StartTip } from "./start-tip";

const GROUPS: [DueGroup, string, string][] = [
  ["overdue", "已逾期", "overdue"],
  ["today", "今天", "today"],
  ["week", "7 天内", ""],
  ["later", "以后", ""],
  ["none", "无截止时间", ""],
];

/** Tasks that can be done now, grouped by when they are due. */
export function NextBody({ now }: { now: Date }) {
  const { board } = useView();
  const { linger, leaving } = useMarks();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const folders = useNextFolders(board);
  const all = folders.includes(null);
  const ready = readyIn(board, folders);
  const lingering = [...linger.keys()].map((id) => board.task(id)).filter((t): t is Task => !!t && t.done_at !== null);
  // The deadline of a task after it that it is to make (docs/design.md#后续的截止); one lingering, as
  // it was listed by.
  const by = (t: Task) => (t.done_at === null ? board.dueBy().get(t.id) : linger.get(t.id));
  const groups = new Map<DueGroup, Task[]>();
  const closed = useClosedGroups(board.me);
  const chains = useChains(board);
  for (const t of board.byDue([...ready, ...lingering], (t) => by(t) ?? t)) {
    const due = (by(t) ?? t).due;
    const g = due ? dueKind(due, now) : "none";
    groups.set(g, [...(groups.get(g) ?? []), t]);
  }
  // Every job not done yet, wherever it was submitted.
  const jobs = board.snapshot.ai_jobs;
  return (
    <>
      <StartTip ready={ready} now={now} />
      {jobs.length > 0 && (
        <section>
          <h2 className="group-title">AI 整理</h2>
          <div className="group">
            <JobRows jobs={jobs} board={board} path />
          </div>
        </section>
      )}
      {!groups.size && !jobs.length && all && <p className="empty">暂无可做的事项。可在文件夹中添加，或让 agent 拆分计划。</p>}
      {!groups.size && !all && (
        <div className="empty">
          <p>这些文件夹里现在没有能做的事</p>
          <button className="text-btn" onClick={() => setNextFolders(board.me, [null])}>
            看全部
          </button>
        </div>
      )}
      {GROUPS.filter(([g]) => groups.has(g)).map(([g, label, cls]) => {
        const tasks = groups.get(g)!;
        const open = !closed.has(g);
        return (
          <section key={g}>
            <h2 className={`group-title ${cls}`}>
              <button className="fold-title" aria-expanded={open} onClick={() => setGroupClosed(board.me, g, open)}>
                {label}
                {!open && <span className="fold-count">{tasks.length} 项</span>}
                <span className="chev">
                  <Icon of={ChevronDown} size={16} />
                </span>
              </button>
            </h2>
            <Fold open={open}>
              <div className="group">
                {tasks.map((t) => {
                  const follows = t.done_at === null && board.after(t).some((a) => a.done_at === null);
                  const unfolded = follows && chains.open.has(t.id);
                  return (
                    <Fragment key={t.id}>
                      <TaskRow
                        task={t}
                        board={board}
                        now={now}
                        path
                        step
                        dueBy={by(t)}
                        leaving={leaving.has(t.id) && t.done_at !== null}
                        selected={selected === t.id}
                        chain={follows ? { open: unfolded, toggle: () => chains.toggle(t) } : undefined}
                        actions={actions}
                      />
                      <Fold open={unfolded}>
                        <Chain task={t} board={board} now={now} selected={selected} actions={actions} />
                      </Fold>
                    </Fragment>
                  );
                })}
              </div>
            </Fold>
          </section>
        );
      })}
    </>
  );
}

/** Which folders “接下来” lists, and picking them, in its title row (docs/ui.md#接下来的文件夹筛选). */
export function NextFilter() {
  const { board } = useView();
  const folders = useNextFolders(board);
  // While picking, none ticked is a step on the way to ticking some once the root is unticked; closed
  // so, it is all of them again.
  const [picking, setPicking] = useState<(FolderId | null)[] | null>(null);
  const pick = (to: (FolderId | null)[]) => {
    setPicking(to);
    if (to.length) setNextFolders(board.me, to);
  };
  const close = () => {
    if (!picking?.length) setNextFolders(board.me, [null]);
    setPicking(null);
  };
  const all = folders.includes(null);
  const names = folders.map((f) => board.folderName(f)).join("、");
  return (
    <>
      <Tip label={all ? "筛选文件夹" : `筛选文件夹：${names}`}>
        <button className={all ? "icon-btn" : "next-filter"} aria-haspopup="dialog" onClick={() => setPicking(folders)}>
          <Icon of={ListFilter} size={all ? undefined : 18} />
          {!all && <span>{folders.length > 2 ? `${folders.length} 个文件夹` : names}</span>}
        </button>
      </Tip>
      {/* Without those deleted or no longer shared meanwhile. */}
      {picking && <FolderChecks title="筛选文件夹" chosen={picking.filter((f) => board.hasFolder(f))} onChange={pick} onClose={close} />}
    </>
  );
}

/** A folder's subfolders and tasks. The root always lists its folders, with a row to make one. */
export function FolderBody({ folder, now }: { folder: FolderId | null; now: Date }) {
  const { board } = useView();
  const { linger, leaving } = useMarks();
  const marking = useMarking();
  const [showing, revealed] = useShowing(folder);
  const keep = useKeepDone();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const [doneOpen, setDoneOpen] = useState(false);
  const subs = board.subfolders(folder);
  // Pending tasks in dependency order, with the `keep` most recently completed in place among them.
  // Those just completed stay a moment besides; when they fold away, as many of the oldest shown fold
  // instead, which are they themselves when none are kept.
  const finished = board.tasksIn(folder).filter((t) => t.done_at !== null);
  const count = keep + finished.filter((t) => linger.has(t.id)).length;
  const shown = new Set(finished.slice(0, count).map((t) => t.id));
  const folding = new Set(finished.slice(count - finished.filter((t) => leaving.has(t.id)).length, count).map((t) => t.id));
  const open = board.order(
    board.contents(folder).filter((t) => t.done_at === null || shown.has(t.id)),
    (t) => t.done_at === null,
  );
  const done = finished.filter((t) => !shown.has(t.id));
  // A task to show here: in the middle, flashing, out of the folded completed ones if it is one of them.
  if (showing && !doneOpen && done.some((t) => t.id === showing.id)) setDoneOpen(true);
  const show = showing?.id;
  useEffect(() => {
    if (!show) return;
    document.querySelector(`[data-task="${show}"]`)?.scrollIntoView({ block: "center" });
    marking.flash([show]);
    revealed();
  }, [show, marking, revealed]);
  const jobs = board.snapshot.ai_jobs.filter((j) => j.folder_id === folder);
  // On a computer, the folders others share are in the sidebar.
  const wide = useWide();
  const theirs = folder === null && !wide ? board.sharedWithMe() : [];
  const [, navigate] = useLocation();
  useListed(doneOpen ? [...open, ...done] : open);
  return (
    <>
      {(subs.length > 0 || folder === null) && (
        <>
          <h2 className="group-title">文件夹</h2>
          <FolderList folder={folder} folders={subs} />
        </>
      )}
      {theirs.length > 0 && (
        <>
          <h2 className="group-title">共享给我</h2>
          <div className="group">
            {theirs.map((f) => (
              <FolderRow key={f.id} folder={f} board={board} onOpen={() => navigate(folderUrl(f.id))} />
            ))}
          </div>
        </>
      )}
      {open.length + done.length + jobs.length > 0 ? (
        <>
          <h2 className="group-title">
            <span>事项</span>
            <span>{open.filter((t) => board.status(t) === "ready").length} 项可做</span>
          </h2>
          <div className="group">
            {jobs.length > 0 && <JobRows jobs={jobs} board={board} />}
            {open.length > 0 && <RailList tasks={open} folding={folding} board={board} now={now} selected={selected} actions={actions} />}
            {done.length > 0 && (
              <button className={`row done-toggle${doneOpen ? " open" : ""}`} onClick={() => setDoneOpen(!doneOpen)}>
                <div className="row-inner">
                  已完成 {done.length} 项
                  <span className="chev">
                    <Icon of={ChevronRight} size={18} />
                  </span>
                </div>
              </button>
            )}
            <Fold open={doneOpen} instant={showing !== null}>
              {done.map((t) => (
                <TaskRow key={t.id} task={t} board={board} now={now} batchable selected={selected === t.id} actions={actions} />
              ))}
            </Fold>
          </div>
        </>
      ) : (
        subs.length === 0 && <p className="empty">暂无事项。可在下方输入，或让 agent 拆分计划。</p>
      )}
    </>
  );
}

/** The folders in `folder`, which can be dragged into another order or into one another; in the root,
 * also a row to make one. */
function FolderList({ folder, folders }: { folder: FolderId | null; folders: Folder[] }) {
  const { board } = useView();
  const [, navigate] = useLocation();
  const newFolder = useNewFolder();
  const place = usePlaceFolder();
  const list = useRef<HTMLDivElement>(null);
  useRowDrag(list, ".folder-row", {
    places: (from) => folders.map((_, at) => ({ at, rows: () => moved(folders.length, from, at) })),
    drop: (from, to) => place(folders[from].id, folder, folders.filter((_, i) => i !== from)[to]?.id ?? null),
    canInto: (from, into) => into !== folder && board.canMove(folders[from].id, into),
    into: (from, into) => place(folders[from].id, into, null),
  });
  return (
    <div className="group reorderable" ref={list}>
      {folders.map((f) => (
        <FolderRow key={f.id} folder={f} board={board} onOpen={() => navigate(folderUrl(f.id))} />
      ))}
      {folder === null && (
        <button className="row add-row" onClick={() => newFolder(null)}>
          <div className="row-inner">
            <Icon of={FolderPlus} />
            新建文件夹
          </div>
        </button>
      )}
    </div>
  );
}

export function FolderViewSeg() {
  const [view, setView] = useFolderView();
  return (
    <Seg
      label="视图"
      items={[
        ["list", "列表", List],
        ["graph", "关系图", Workflow],
      ]}
      value={view}
      onChange={setView}
    />
  );
}

// The graph pulls in React Flow and ELK, so it loads when first shown.
const Graph = lazy(() => import("./Graph"));

export function FolderGraph(props: { folder: FolderId | null; across: boolean; now: Date }) {
  return (
    <Suspense fallback={<div className="graph-area" />}>
      <Graph key={props.folder ?? "root"} {...props} />
    </Suspense>
  );
}

/** The user's completed tasks from every folder, the latest first, by the day they were completed. */
export function DoneBody({ now }: { now: Date }) {
  const { board } = useView();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const days = new Map<string, Task[]>();
  const done = board.snapshot.tasks.filter((t) => t.done_at !== null && board.mine(t)).sort((a, b) => b.done_at! - a.done_at!);
  useListed(done);
  for (const t of done) {
    const day = formatDay(t.done_at!, now);
    days.set(day, [...(days.get(day) ?? []), t]);
  }
  if (!days.size) return <p className="empty">暂无已完成的事项。</p>;
  return [...days].map(([day, tasks]) => (
    <section key={day}>
      <h2 className="group-title">{day}</h2>
      <div className="group">
        {tasks.map((t) => (
          <TaskRow key={t.id} task={t} board={board} now={now} path batchable selected={selected === t.id} actions={actions} />
        ))}
      </div>
    </section>
  ));
}

export function SearchResults({ query, now }: { query: string; now: Date }) {
  const { board } = useView();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const q = query.trim().toLowerCase();
  const hits = board
    .sort(board.snapshot.tasks.filter((t) => matches(t, q)))
    .sort((a, b) => Number(a.done_at !== null) - Number(b.done_at !== null));
  if (!hits.length) return <p className="empty">未找到包含“{query.trim()}”的事项。</p>;
  return (
    <>
      <h2 className="group-title">{hits.length} 个结果</h2>
      <div className="group">
        {hits.map((t) => (
          <TaskRow key={t.id} task={t} board={board} now={now} path selected={selected === t.id} actions={actions} />
        ))}
      </div>
    </>
  );
}
