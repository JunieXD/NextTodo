// The bodies of the main pages: what is next, what is done, a folder (the root too), search results;
// and the quick-add bar.

import { ArrowUp, ChevronDown, ChevronRight, ChevronUp, Folder as FolderIcon, FolderPlus, List, Sparkles, Workflow } from "lucide-react";
import { Dialog } from "radix-ui";
import { lazy, Suspense, useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation, useSearchParams } from "wouter";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { lastKey } from "../domain/apply";
import { type Board, matches } from "../domain/board";
import { aiReady } from "../ai";
import { type DueKind, dueKind, formatDay, localTime } from "../domain/dates";
import { rekey } from "../domain/reorder";
import { checkJobText, checkTitle } from "../domain/text";
import { useCommit, useSession, useView } from "../session";
import { draft, keepDraft } from "../sync/drafts";
import { useBack } from "../ui/back";
import { RootContext, Tip, useFeedback } from "../ui/feedback";
import { Seg } from "../ui/controls";
import { Icon } from "../ui/icons";
import { composing } from "../ui/keys";
import { useLeave } from "../ui/leave";
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
              <TaskRow key={t.id} task={t} board={board} now={now} path open selected={selected === t.id} actions={actions} />
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

const AI_MODE = "nexttodo.aiMode";

/** Adds tasks by typing, or, switched to AI organizing with ✦, has AI organize what is written into
 * tasks; that text can be expanded to write at length. In a folder (`null` for the root) it adds
 * there; elsewhere to a folder picked beside it, the root by default. */
export function QuickAdd({ folder }: { folder?: FolderId | null }) {
  const { board } = useView();
  const commit = useCommit();
  const { snack } = useFeedback();
  const { user } = useSession();
  const openSettings = useContext(OpenSettings);
  const place = folder === undefined ? "next" : (folder ?? "root");
  const [title, setTitle] = useState(() => draft(user.id, place));
  const edit = (text: string) => {
    setTitle(text);
    keepDraft(user.id, place, text);
  };
  // The device remembers the mode last used.
  const [ai, setAi] = useState(() => localStorage.getItem(AI_MODE) === "on");
  const [target, setTarget] = useState<FolderId | null>(null);
  const [picking, setPicking] = useState(false);
  const [tall, setTall] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const into = folder !== undefined ? folder : target && board.folder(target) ? target : null;
  const area = useRef<HTMLTextAreaElement>(null);
  const focus = () => area.current?.focus();
  // The app's widget opens `/?add` to type a new task straight away.
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const asked = params.has("add");
  useEffect(() => {
    if (!asked) return;
    focus();
    navigate(location, { replace: true });
  }, [asked, location, navigate]);
  // As tall as the text, up to the height the style allows, and then it scrolls; past three lines
  // AI organizing's can be expanded. Again when its width changes, as on turning the phone, which
  // moves where the lines break.
  useLayoutEffect(() => {
    const el = area.current!;
    const fit = () => {
      el.style.height = "auto";
      el.style.height = `${el.scrollHeight}px`;
      const style = getComputedStyle(el);
      setTall((el.scrollHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom)) / parseFloat(style.lineHeight) > 3);
    };
    fit();
    let width = el.clientWidth;
    const resized = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      fit();
    });
    resized.observe(el);
    return () => resized.disconnect();
  }, [title]);

  const switchMode = async () => {
    if (!ai && !(await aiReady(user.id))) {
      snack("先选好 AI 整理用的模型");
      return openSettings("ai");
    }
    localStorage.setItem(AI_MODE, ai ? "off" : "on");
    setAi(!ai);
    requestAnimationFrame(focus);
  };

  /** Hands the text to AI organizing; whether it went. */
  const organize = () => {
    const c = checkJobText(title);
    if (!c.ok) {
      snack(c.error);
      return false;
    }
    const op: Op = { op: "ai_job.create", id: uuidv7(), text: c.value, local_time: localTime(new Date()), ...(into && { folder_id: into }) };
    if (!commit([op])) return false;
    edit("");
    snack("已交给 AI 整理");
    return true;
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;
    if (ai) return void organize();
    const c = checkTitle(title);
    if (!c.ok) return snack([...title.trim()].length > 200 ? `${c.error}；长段文字可以点 ✦ 交给 AI 整理` : c.error);
    const op: Op = { op: "task.create", id: uuidv7(), title: c.value, sort_key: lastKey(board.contents(into).map((t) => t.sort_key)), ...(into && { folder_id: into }) };
    if (!commit([op])) return;
    edit("");
    snack(`已添加“${c.value}”`);
  };

  // A title is one line, so Enter adds it; AI organizing's text has many, so Enter starts one and
  // Ctrl or ⌘ + Enter submits. Not while an input method is still composing.
  const enter = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && (!ai || e.ctrlKey || e.metaKey) && !composing(e)) {
      e.preventDefault();
      e.currentTarget.form!.requestSubmit();
    }
  };
  const pick = folder === undefined && (
    <button type="button" className="qa-btn folder" onClick={() => setPicking(true)}>
      <Icon of={FolderIcon} size={16} />
      <span>{board.folderName(into)}</span>
    </button>
  );
  const send = (
    <Tip label={ai ? "交给 AI 整理" : "添加"}>
      <button type="submit" className="qa-btn icon send" disabled={!title.trim()}>
        <Icon of={ArrowUp} size={18} width={2.2} />
      </button>
    </Tip>
  );

  return (
    <>
      <form className="quick-add" onSubmit={submit}>
        <div className="qa-field">
          {/* A title pasted on many lines is joined into one. */}
          <textarea
            ref={area}
            rows={1}
            value={title}
            onChange={(e) => edit(ai ? e.target.value : e.target.value.replace(/[\r\n]/g, ""))}
            onKeyDown={enter}
            placeholder={ai ? "交给 AI 整理" : "添加事项"}
            autoComplete="off"
            enterKeyHint={ai ? undefined : "done"}
            aria-label={ai ? "交给 AI 整理的内容" : "新事项标题"}
          />
        </div>
        <div className="qa-side">
          {ai && tall && (
            <Tip label="展开编辑">
              <button type="button" className="qa-btn icon expand" onClick={() => setExpanded(true)}>
                <Icon of={ChevronUp} size={18} />
              </button>
            </Tip>
          )}
          <div className="qa-btns">
            {/* Not taking the focus from the text: on a phone the keyboard would go down and come up again. */}
            <Tip label="AI 整理">
              <button
                type="button"
                className={`qa-btn icon${ai ? " on" : ""}`}
                aria-pressed={ai}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => void switchMode()}
              >
                <Icon of={Sparkles} size={16} />
              </button>
            </Tip>
            {pick}
            {send}
          </div>
        </div>
      </form>
      {expanded && (
        <Editor text={title} onEdit={edit} onEnter={enter} onSubmit={() => organize() && setExpanded(false)} onClose={() => setExpanded(false)}>
          {pick}
          {send}
        </Editor>
      )}
      {picking && <FolderPicker title="添加到" current={into} onPick={setTarget} onClose={() => setPicking(false)} />}
    </>
  );
}

/** AI organizing's text on a page of its own, to write at length: full screen on a phone, a large
 * window on a computer. Kept outside the bar's form, which would otherwise get its submits too. */
function Editor(props: {
  text: string;
  onEdit(text: string): void;
  onEnter(e: React.KeyboardEvent<HTMLTextAreaElement>): void;
  onSubmit(): void;
  onClose(): void;
  children: React.ReactNode;
}) {
  const root = useContext(RootContext);
  const area = useRef<HTMLTextAreaElement>(null);
  const scrim = useRef<HTMLDivElement>(null);
  const page = useRef<HTMLDivElement>(null);
  useLeave([scrim, page]);
  useBack("dialog", props.onClose);
  return (
    <Dialog.Root open onOpenChange={(o) => !o && props.onClose()}>
      <Dialog.Portal container={root}>
        <Dialog.Overlay ref={scrim} className="modal-scrim enter" />
        <Dialog.Content
          ref={page}
          className="editor enter"
          aria-describedby={undefined}
          // Goes on writing where the text ends. Without scrolling: the page is still sliding in, and
          // the app would scroll to where it is now.
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            const el = area.current!;
            el.focus({ preventScroll: true });
            el.setSelectionRange(el.value.length, el.value.length);
          }}
          asChild
        >
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (props.text.trim()) props.onSubmit();
            }}
          >
            <div className="modal-head">
              <Dialog.Title asChild>
                <h3>AI 整理</h3>
              </Dialog.Title>
              <Tip label="收起">
                <Dialog.Close className="icon-btn">
                  <Icon of={ChevronDown} />
                </Dialog.Close>
              </Tip>
            </div>
            <textarea ref={area} value={props.text} onChange={(e) => props.onEdit(e.target.value)} onKeyDown={props.onEnter} placeholder="交给 AI 整理" aria-label="交给 AI 整理的内容" />
            <div className="editor-foot">{props.children}</div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
