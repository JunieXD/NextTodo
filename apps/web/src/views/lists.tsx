// The bodies of the main pages: what is next, a folder, the folder root, search results; and the
// quick-add bar.

import { ChevronRight, Folder as FolderIcon, FolderPlus, List, Plus, Workflow } from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation, useSearchParams } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { lastKey } from "../domain/apply";
import type { Board } from "../domain/board";
import { type DueKind, dueKind } from "../domain/dates";
import { checkFolderName, checkTitle } from "../domain/text";
import { useCommit, useSession, useView } from "../session";
import { useFeedback } from "../ui/feedback";
import { Seg } from "../ui/controls";
import { Chain, Icon } from "../ui/icons";
import { useTaskActions } from "./actions";
import { useMarks } from "./marks";
import { folderUrl, useFolderView, useSelection } from "./nav";
import { FolderPicker } from "./pickers";
import { FolderRow, RailList, TaskRow } from "./rows";

const GROUPS: [DueKind | "none", string, string][] = [
  ["overdue", "已逾期", "overdue"],
  ["today", "今天", "today"],
  ["week", "7 天内", ""],
  ["later", "以后", ""],
  ["none", "没有截止时间", ""],
];

/** Tasks that can be done now, grouped by when they are due. */
export function NextBody({ now }: { now: Date }) {
  const { board } = useView();
  const { linger } = useMarks();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const ready = board.next();
  const lingering = [...linger].map((id) => board.task(id)).filter((t): t is Task => !!t && t.done_at !== null);
  const groups = new Map<string, Task[]>();
  for (const t of board.byDue([...ready, ...lingering])) {
    const g = t.due ? dueKind(t.due, now) : "none";
    groups.set(g, [...(groups.get(g) ?? []), t]);
  }
  if (!groups.size) return <p className="empty">现在没有能做的事。去文件夹里添加，或者让 agent 帮你拆一个计划。</p>;
  return GROUPS.filter(([g]) => groups.has(g)).map(([g, label, cls]) => (
    <section key={g}>
      <h2 className={`group-title ${cls}`}>{label}</h2>
      <div className="group">
        {groups.get(g)!.map((t) => (
          <TaskRow key={t.id} task={t} board={board} now={now} path selected={selected === t.id} actions={actions} />
        ))}
      </div>
    </section>
  ));
}

/** Pending tasks of a folder in dependency order, with those just completed kept in place for a moment. */
function openTasks(board: Board, folder: FolderId | null, linger: Set<TaskId>): Task[] {
  return board.order(
    board.contents(folder).filter((t) => t.done_at === null || linger.has(t.id)),
    (t) => t.done_at === null,
  );
}

export function FolderBody({ folder, now }: { folder: FolderId; now: Date }) {
  const { board } = useView();
  const { linger } = useMarks();
  const { selected } = useSelection();
  const [, navigate] = useLocation();
  const actions = useTaskActions();
  const [doneOpen, setDoneOpen] = useState(false);
  const subs = board.subfolders(folder);
  const open = openTasks(board, folder, linger);
  const done = board.tasksIn(folder).filter((t) => t.done_at !== null && !linger.has(t.id));
  return (
    <>
      {subs.length > 0 && (
        <>
          <h2 className="group-title">文件夹</h2>
          <div className="group">
            {subs.map((f) => (
              <FolderRow key={f.id} folder={f} board={board} onOpen={() => navigate(folderUrl(f.id))} />
            ))}
          </div>
        </>
      )}
      {open.length + done.length > 0 ? (
        <>
          <h2 className="group-title">
            <span>事项</span>
            <span>{open.filter((t) => board.status(t) === "ready").length} 项可做</span>
          </h2>
          <div className="group">
            {open.length > 0 && <RailList tasks={open} board={board} now={now} selected={selected} actions={actions} />}
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
            {doneOpen && done.map((t) => <TaskRow key={t.id} task={t} board={board} now={now} selected={selected === t.id} actions={actions} />)}
          </div>
        </>
      ) : (
        subs.length === 0 && <p className="empty">还没有事项。在下面输入，或者让 agent 帮你拆一个计划。</p>
      )}
    </>
  );
}

export function FolderViewSeg() {
  const [view, setView] = useFolderView();
  return (
    <Seg
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

export function FolderGraph(props: { folder: FolderId; across: boolean; now: Date }) {
  return (
    <Suspense fallback={<div className="graph-area" />}>
      <Graph key={props.folder} {...props} />
    </Suspense>
  );
}

/** The top level: folders, and tasks not in any folder. */
export function RootBody({ now }: { now: Date }) {
  const { board } = useView();
  const { linger } = useMarks();
  const { selected } = useSelection();
  const [, navigate] = useLocation();
  const actions = useTaskActions();
  const newFolder = useNewFolder();
  const own = openTasks(board, null, linger);
  return (
    <>
      <h2 className="group-title">文件夹</h2>
      <div className="group">
        {board.subfolders(null).map((f) => (
          <FolderRow key={f.id} folder={f} board={board} onOpen={() => navigate(folderUrl(f.id))} />
        ))}
        <button className="row add-row" onClick={() => newFolder(null)}>
          <div className="row-inner">
            <Icon of={FolderPlus} />
            新建文件夹
          </div>
        </button>
      </div>
      {own.length > 0 && (
        <>
          <h2 className="group-title">不在文件夹里</h2>
          <div className="group">
            {own.map((t) => (
              <TaskRow key={t.id} task={t} board={board} now={now} selected={selected === t.id} actions={actions} />
            ))}
          </div>
        </>
      )}
    </>
  );
}

export function SearchResults({ query, now }: { query: string; now: Date }) {
  const { board } = useView();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const q = query.trim().toLowerCase();
  const hits = board
    .sort(board.snapshot.tasks.filter((t) => t.title.toLowerCase().includes(q) || t.description.toLowerCase().includes(q)))
    .sort((a, b) => Number(a.done_at !== null) - Number(b.done_at !== null));
  if (!hits.length) return <p className="empty">没有找到包含“{query.trim()}”的事项。</p>;
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

/** Asks for a name and makes a folder inside `parent`. */
export function useNewFolder() {
  const { prompt, snack } = useFeedback();
  const commit = useCommit();
  const { store } = useSession();
  return async (parent: FolderId | null) => {
    const name = await prompt({ title: "新建文件夹", label: "名称", ok: "新建", check: checkFolderName });
    if (name === null) return;
    const sort_key = lastKey(store.getView().board.subfolders(parent).map((f) => f.sort_key));
    if (commit([{ op: "folder.create", id: uuidv7(), name, sort_key, ...(parent && { parent_id: parent }) }])) snack(`已新建“${name}”`);
  };
}

/** Adds tasks by typing. In a folder it can chain each new task after the previous one; elsewhere
 * it adds to a folder picked beside it, the root by default. */
export function QuickAdd({ folder }: { folder?: FolderId }) {
  const { board } = useView();
  const commit = useCommit();
  const { snack } = useFeedback();
  const [title, setTitle] = useState("");
  const [chain, setChain] = useState(false);
  const [target, setTarget] = useState<FolderId | null>(null);
  const [picking, setPicking] = useState(false);
  const last = useRef<TaskId | null>(null);
  const into = folder ?? (target && board.folder(target) ? target : null);
  // The app's widget opens `/?add` to type a new task straight away.
  const input = useRef<HTMLInputElement>(null);
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const asked = params.has("add");
  useEffect(() => {
    if (!asked) return;
    input.current!.focus();
    navigate(location, { replace: true });
  }, [asked, location, navigate]);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;
    const c = checkTitle(title);
    if (!c.ok) return snack(c.error);
    const id = uuidv7();
    const ops: Op[] = [{ op: "task.create", id, title: c.value, sort_key: lastKey(board.contents(into).map((t) => t.sort_key)), ...(into && { folder_id: into }) }];
    const prev = chain && last.current ? board.task(last.current) : undefined;
    const after = prev && prev.folder_id === into && prev.done_at === null ? prev : undefined;
    if (after) ops.push({ op: "dep.add", before: after.id, after: id });
    if (!commit(ops)) return;
    last.current = chain ? id : null;
    setTitle("");
    snack(after ? `已添加，接在“${after.title}”后面` : `已添加“${c.value}”`);
  };

  return (
    <form className="quick-add" onSubmit={submit}>
      <span className="plus">
        <Icon of={Plus} size={22} width={2} />
      </span>
      <input ref={input} value={title} onChange={(e) => setTitle(e.target.value)} placeholder={into ? "添加事项" : "记一件事"} autoComplete="off" enterKeyHint="done" aria-label="新事项标题" />
      {folder === undefined ? (
        <button type="button" className="qa-btn" onClick={() => setPicking(true)}>
          <Icon of={FolderIcon} size={16} />
          {into ? board.folder(into)!.name : "根目录"}
        </button>
      ) : (
        <button type="button" className={`qa-btn${chain ? " on" : ""}`} aria-pressed={chain} onClick={() => ((last.current = null), setChain(!chain))}>
          <Icon of={Chain} size={16} />
          依次排队
        </button>
      )}
      {picking && <FolderPicker title="记到哪里" current={into} onPick={setTarget} onClose={() => setPicking(false)} />}
    </form>
  );
}
