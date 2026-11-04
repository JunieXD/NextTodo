// The bodies of the main pages: what is next, what is done, a folder (the root too), search results;
// and the quick-add bar.

import { ArrowUp, ChevronRight, Folder as FolderIcon, FolderPlus, List, Plus, Sparkles, Workflow } from "lucide-react";
import { lazy, Suspense, useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation, useSearchParams } from "wouter";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { lastKey } from "../domain/apply";
import type { Board } from "../domain/board";
import { aiReady } from "../ai";
import { type DueKind, dueKind, formatDay, localTime } from "../domain/dates";
import { rekey } from "../domain/reorder";
import { checkJobText, checkTitle } from "../domain/text";
import { useCommit, useSession, useView } from "../session";
import { draft, keepDraft } from "../sync/drafts";
import { useFeedback } from "../ui/feedback";
import { Seg } from "../ui/controls";
import { useWide } from "../ui/hooks";
import { Chain, Icon } from "../ui/icons";
import { useTaskActions } from "./actions";
import { useRowDrag } from "./drag";
import { useNewFolder } from "./FolderMenu";
import { useMarks } from "./marks";
import { OpenSettings, folderUrl, useFolderView, useSelection } from "./nav";
import { JobRows } from "./jobs";
import { FolderPicker } from "./pickers";
import { FolderRow, RailList, TaskRow } from "./rows";

const GROUPS: [DueKind | "none", string, string][] = [
  ["overdue", "已逾期", "overdue"],
  ["today", "今天", "today"],
  ["week", "7 天内", ""],
  ["later", "以后", ""],
  ["none", "无截止时间", ""],
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
  // Every job not done yet, wherever it was submitted.
  const jobs = board.snapshot.ai_jobs;
  return (
    <>
      {jobs.length > 0 && (
        <section>
          <h2 className="group-title">AI 整理</h2>
          <div className="group">
            <JobRows jobs={jobs} board={board} path />
          </div>
        </section>
      )}
      {!groups.size && !jobs.length && <p className="empty">暂无可做的事项。可在文件夹中添加，或让 agent 拆分计划。</p>}
      {GROUPS.filter(([g]) => groups.has(g)).map(([g, label, cls]) => (
        <section key={g}>
          <h2 className={`group-title ${cls}`}>{label}</h2>
          <div className="group">
            {groups.get(g)!.map((t) => (
              <TaskRow key={t.id} task={t} board={board} now={now} path selected={selected === t.id} actions={actions} />
            ))}
          </div>
        </section>
      ))}
    </>
  );
}

/** Pending tasks of a folder in dependency order, with those just completed kept in place for a moment. */
function openTasks(board: Board, folder: FolderId | null, linger: Set<TaskId>): Task[] {
  return board.order(
    board.contents(folder).filter((t) => t.done_at === null || linger.has(t.id)),
    (t) => t.done_at === null,
  );
}

/** A folder's subfolders and tasks. The root always lists its folders, with a row to make one. */
export function FolderBody({ folder, now }: { folder: FolderId | null; now: Date }) {
  const { board } = useView();
  const { linger } = useMarks();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const [doneOpen, setDoneOpen] = useState(false);
  const subs = board.subfolders(folder);
  const open = openTasks(board, folder, linger);
  const done = board.tasksIn(folder).filter((t) => t.done_at !== null && !linger.has(t.id));
  const jobs = board.snapshot.ai_jobs.filter((j) => j.folder_id === folder);
  return (
    <>
      {(subs.length > 0 || folder === null) && (
        <>
          <h2 className="group-title">文件夹</h2>
          <FolderList folder={folder} folders={subs} />
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
        subs.length === 0 && <p className="empty">暂无事项。可在下方输入，或让 agent 拆分计划。</p>
      )}
    </>
  );
}

/** The folders in `folder`, which can be dragged into another order; in the root, also a row to make
 * one. */
function FolderList({ folder, folders }: { folder: FolderId | null; folders: Folder[] }) {
  const { board } = useView();
  const commit = useCommit();
  const [, navigate] = useLocation();
  const newFolder = useNewFolder();
  const list = useRef<HTMLDivElement>(null);
  useRowDrag(list, ".folder-row", {
    range: () => ({ min: 0, max: folders.length - 1 }),
    drop(from, to) {
      if (to === from) return;
      const wanted = folders.filter((_, i) => i !== from);
      wanted.splice(to, 0, folders[from]);
      commit(rekey(wanted, folders[from].id).map((k): Op => ({ op: "folder.update", ...k })));
    },
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

/** Completed tasks from every folder, the latest first, by the day they were completed. */
export function DoneBody({ now }: { now: Date }) {
  const { board } = useView();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const days = new Map<string, Task[]>();
  for (const t of board.snapshot.tasks.filter((t) => t.done_at !== null).sort((a, b) => b.done_at! - a.done_at!)) {
    const day = formatDay(t.done_at!, now);
    days.set(day, [...(days.get(day) ?? []), t]);
  }
  if (!days.size) return <p className="empty">暂无已完成的事项。</p>;
  return [...days].map(([day, tasks]) => (
    <section key={day}>
      <h2 className="group-title">{day}</h2>
      <div className="group">
        {tasks.map((t) => (
          <TaskRow key={t.id} task={t} board={board} now={now} path selected={selected === t.id} actions={actions} />
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
    .sort(board.snapshot.tasks.filter((t) => t.title.toLowerCase().includes(q) || t.description.toLowerCase().includes(q)))
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

const AI_MODE = "nexttodo.aiMode";

/** Adds tasks by typing, or, switched to AI organizing with ✦, has AI organize what is written into
 * tasks. In a folder (`null` for the root) it can chain each new task after the previous one;
 * elsewhere it adds to a folder picked beside it, the root by default. */
export function QuickAdd({ folder }: { folder?: FolderId | null }) {
  const { board } = useView();
  const commit = useCommit();
  const { snack } = useFeedback();
  const { user } = useSession();
  const openSettings = useContext(OpenSettings);
  const wide = useWide();
  const place = folder === undefined ? "next" : (folder ?? "root");
  const [title, setTitle] = useState(() => draft(user.id, place));
  const edit = (text: string) => {
    setTitle(text);
    keepDraft(user.id, place, text);
  };
  // The device remembers the mode last used.
  const [ai, setAi] = useState(() => localStorage.getItem(AI_MODE) === "on");
  const [chain, setChain] = useState(false);
  const [target, setTarget] = useState<FolderId | null>(null);
  const [picking, setPicking] = useState(false);
  const last = useRef<TaskId | null>(null);
  const into = folder !== undefined ? folder : target && board.folder(target) ? target : null;
  const input = useRef<HTMLInputElement>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const focus = () => (input.current ?? area.current)?.focus();
  // The app's widget opens `/?add` to type a new task straight away.
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const asked = params.has("add");
  useEffect(() => {
    if (!asked) return;
    focus();
    navigate(location, { replace: true });
  }, [asked, location, navigate]);
  // As tall as the text, up to the height the style allows, and then it scrolls.
  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [title, ai]);

  const switchMode = async () => {
    if (!ai && !(await aiReady(user.id))) {
      snack("先选好 AI 整理用的模型");
      return openSettings("ai");
    }
    localStorage.setItem(AI_MODE, ai ? "off" : "on");
    setAi(!ai);
    requestAnimationFrame(focus);
  };

  const organize = () => {
    const c = checkJobText(title);
    if (!c.ok) return snack(c.error);
    const op: Op = { op: "ai_job.create", id: uuidv7(), text: c.value, local_time: localTime(new Date()), ...(into && { folder_id: into }) };
    if (!commit([op])) return;
    edit("");
    snack("已交给 AI 整理");
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;
    if (ai) return organize();
    const c = checkTitle(title);
    if (!c.ok) return snack([...title.trim()].length > 200 ? `${c.error}；长段文字可以点 ✦ 交给 AI 整理` : c.error);
    const id = uuidv7();
    const ops: Op[] = [{ op: "task.create", id, title: c.value, sort_key: lastKey(board.contents(into).map((t) => t.sort_key)), ...(into && { folder_id: into }) }];
    const prev = chain && last.current ? board.task(last.current) : undefined;
    const after = prev && prev.folder_id === into && prev.done_at === null ? prev : undefined;
    if (after) ops.push({ op: "dep.add", before: after.id, after: id });
    if (!commit(ops)) return;
    last.current = chain ? id : null;
    edit("");
    snack(after ? `已添加，排在“${after.title}”之后` : `已添加“${c.value}”`);
  };

  return (
    <form className={`quick-add${ai ? " ai" : ""}`} onSubmit={submit}>
      <span className="plus">
        <Icon of={ai ? Sparkles : Plus} size={22} width={ai ? 1.8 : 2} />
      </span>
      {ai ? (
        <textarea
          ref={area}
          rows={1}
          value={title}
          onChange={(e) => edit(e.target.value)}
          // On a computer Enter submits and Shift+Enter starts a line; on a phone the key starts a
          // line and the button submits. Not while an input method is still composing.
          onKeyDown={(e) => {
            if (wide && e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              e.currentTarget.form!.requestSubmit();
            }
          }}
          placeholder="写下要做的事，AI 整理成事项"
          aria-label="交给 AI 整理的内容"
        />
      ) : (
        <input ref={input} value={title} onChange={(e) => edit(e.target.value)} placeholder="添加事项" autoComplete="off" enterKeyHint="done" aria-label="新事项标题" />
      )}
      <button type="button" className={`qa-btn icon${ai ? " on" : ""}`} aria-pressed={ai} aria-label="AI 整理" title="AI 整理" onClick={() => void switchMode()}>
        <Icon of={Sparkles} size={16} />
      </button>
      {folder === undefined ? (
        <button type="button" className="qa-btn" onClick={() => setPicking(true)}>
          <Icon of={FolderIcon} size={16} />
          {board.folderName(into)}
        </button>
      ) : (
        !ai && (
          <button type="button" className={`qa-btn${chain ? " on" : ""}`} aria-pressed={chain} onClick={() => ((last.current = null), setChain(!chain))}>
            <Icon of={Chain} size={16} />
            依次排队
          </button>
        )
      )}
      {ai && (
        <button type="submit" className="qa-btn icon send" aria-label="交给 AI 整理" disabled={!title.trim()}>
          <Icon of={ArrowUp} size={18} width={2.2} />
        </button>
      )}
      {picking && <FolderPicker title="添加到" current={into} onPick={setTarget} onClose={() => setPicking(false)} />}
    </form>
  );
}
