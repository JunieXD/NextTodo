// The bodies of the main pages: what is next, what is done, a folder (the root too), search results;
// and the quick-add bar.

import { ArrowUp, ChevronDown, ChevronRight, ChevronUp, Folder as FolderIcon, FolderPlus, List, ListFilter, Sparkles, Workflow } from "lucide-react";
import { Dialog } from "radix-ui";
import { lazy, Suspense, useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation, useSearchParams } from "wouter";
import { NextTodo } from "../mobile";
import { app } from "../api/http";
import type { Due } from "../api/types/Due";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import { lastKey } from "../domain/apply";
import { matches } from "../domain/board";
import { aiReady, setAiMode, useAiMode, useAiVision } from "../ai";
import { day, type DueKind, dueKind, formatDay, inOrder, localTime } from "../domain/dates";
import { checkJobText, checkTitle } from "../domain/text";
import { useCommit, useSession, useView } from "../session";
import { readyIn, setNextFolders, useNextFolders } from "../next-folders";
import { defaultReminders } from "../remind-settings";
import { addOps, filesIn, isPicture, plan, unseen } from "../attach";
import { type Staged, draft, keepDraft, keepStaged, onDrafts, stagedFiles } from "../sync/drafts";
import { dropFiles } from "../sync/files";
import { useBack } from "../ui/back";
import { Fold } from "../ui/fold";
import { RootContext, Tip, useFeedback } from "../ui/feedback";
import { Seg } from "../ui/controls";
import { Icon } from "../ui/icons";
import { composing } from "../ui/keys";
import { useLeave } from "../ui/leave";
import { useWide } from "../ui/hooks";
import { useTaskActions } from "./actions";
import { StagedFiles, useKeepFiles } from "./attachments";
import { useListed } from "./batch";
import { useRowDrag } from "./drag";
import { useNewFolder, usePlaceFolder } from "./FolderMenu";
import { useMarks } from "./marks";
import { folderUrl, settingsUrl, useFolderView, useSelection } from "./nav";
import { JobRows } from "./jobs";
import { useKeepDone } from "./keep-done";
import { FolderChecks, FolderPicker } from "./pickers";
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
  const { linger, leaving } = useMarks();
  const { selected } = useSelection();
  const actions = useTaskActions();
  const folders = useNextFolders(board);
  const all = folders.includes(null);
  const ready = readyIn(board, folders);
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
      {!groups.size && !jobs.length && all && <p className="empty">暂无可做的事项。可在文件夹中添加，或让 agent 拆分计划。</p>}
      {!groups.size && !all && (
        <div className="empty">
          <p>这些文件夹里现在没有能做的事</p>
          <button className="text-btn" onClick={() => setNextFolders(board.me, [null])}>
            看全部
          </button>
        </div>
      )}
      {GROUPS.filter(([g]) => groups.has(g)).map(([g, label, cls]) => (
        <section key={g}>
          <h2 className={`group-title ${cls}`}>{label}</h2>
          <div className="group">
            {groups.get(g)!.map((t) => (
              <TaskRow key={t.id} task={t} board={board} now={now} path step leaving={leaving.has(t.id) && t.done_at !== null} selected={selected === t.id} actions={actions} />
            ))}
          </div>
        </section>
      ))}
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
            <Fold open={doneOpen}>
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
    range: () => ({ min: 0, max: folders.length - 1 }),
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

/** A due date on `date`, with this device's defaults for one without a time. */
function due(date: string): Due {
  const reminders = inOrder(defaultReminders().dated, undefined);
  return { date, ...(reminders.length > 0 && { reminders }) };
}

/** Adds tasks by typing, or, switched to AI organizing with ✦, has AI organize what is written into
 * tasks; that text can be expanded to write at length. Files pasted or dropped in go with what is
 * added. In a folder (`null` for the root) it adds there; elsewhere to a folder picked beside it, the
 * root by default. In the calendar, `on` the day picked there, a task is due that day, with the
 * defaults for a due date; it has no AI organizing, which dates tasks by what the text says. Hidden by
 * `hide`, it keeps what it has, the folder picked and the files still being saved. */
export function QuickAdd({ folder, on, hide = false }: { folder?: FolderId | null; on?: string; hide?: boolean }) {
  const { board } = useView();
  const commit = useCommit();
  const { snack } = useFeedback();
  const { user } = useSession();
  const place = on !== undefined ? "calendar" : folder === undefined ? "next" : (folder ?? "root");
  const [title, setTitle] = useState(() => draft(user.id, place));
  const edit = (text: string) => {
    setTitle(text);
    keepDraft(user.id, place, text);
  };
  const [staged, setStaged] = useState(() => stagedFiles(user.id, place));
  const stage = (files: Staged[]) => {
    setStaged(files);
    keepStaged(user.id, place, files);
  };
  // An AI job turned down comes back here, also from another tab; the box of another tab is kept too.
  useEffect(
    () =>
      onDrafts(() => {
        setTitle(draft(user.id, place));
        setStaged(stagedFiles(user.id, place));
      }),
    [user.id, place],
  );
  const keepFiles = useKeepFiles();
  const addFiles = async (files: File[]) => {
    if (!files.length) return;
    const kept = await keepFiles(plan(files, staged.map((f) => f.name), snack));
    // As they are now: more may have come meanwhile. None once signed out, when the drafts are gone.
    if (kept.length) stage([...stagedFiles(user.id, place), ...kept]);
  };
  const unstage = (f: Staged) => {
    stage(staged.filter((s) => s.id !== f.id));
    void dropFiles(user.id, [f.id]);
  };
  /** What pasting or dropping files does in the box's text. */
  const files = {
    onPaste: (e: React.ClipboardEvent) => {
      const list = filesIn(e.clipboardData);
      if (!list.length) return;
      e.preventDefault();
      void addFiles(list);
    },
    onDragOver: (e: React.DragEvent) => e.dataTransfer.types.includes("Files") && e.preventDefault(),
    onDrop: (e: React.DragEvent) => {
      const list = filesIn(e.dataTransfer);
      if (!list.length) return;
      e.preventDefault();
      void addFiles(list);
    },
  };
  /** Whether the files staged can go; one the server turned down has to be taken out first. */
  const canSend = () => {
    const refused = staged.find((f) => f.error);
    if (refused) snack(`“${refused.name}”上传失败，去掉后再提交`);
    return !refused;
  };
  // The device remembers the mode last used.
  const ai = useAiMode() && on === undefined;
  const vision = useAiVision(user.id, ai && staged.some((f) => isPicture(f.type)));
  const hidden = ai && vision !== undefined ? unseen(staged, vision) : undefined;
  // In “接下来” until one is picked, the first folder it lists, so that a new task shows there; in the
  // calendar, the root.
  const [target, setTarget] = useState<FolderId | null>();
  const listed = useNextFolders(board);
  const [picking, setPicking] = useState(false);
  const [tall, setTall] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const into =
    folder !== undefined
      ? folder
      : target !== undefined && board.hasFolder(target)
        ? target
        : on === undefined
          ? (board.folderTree().find((f) => listed.includes(f)) ?? null)
          : null;
  const when = on && formatDay(day(on).getTime(), new Date());
  const area = useRef<HTMLTextAreaElement>(null);
  const focus = () => area.current?.focus();
  // The Android widget opens `/?add` to type a new task straight away, and so does the desktop app's
  // panel each time it opens: on Android with the keyboard up, which focusing alone does not bring.
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const asked = params.has("add");
  useEffect(() => {
    if (!asked) return;
    focus();
    if (app === "android") void NextTodo.showKeyboard();
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
      return navigate(settingsUrl("ai"));
    }
    await setAiMode(!ai);
    requestAnimationFrame(focus);
  };

  /** Hands the text to AI organizing, with the files staged, in one change: the job starts once they
   * are uploaded. Whether it went. */
  const organize = () => {
    const c = checkJobText(title);
    if (!c.ok) {
      snack(c.error);
      return false;
    }
    if (!canSend()) return false;
    const id = uuidv7();
    const op: Op = { op: "ai_job.create", id, text: c.value, local_time: localTime(new Date()), ...(into && { folder_id: into }) };
    if (!commit([op, ...addOps(staged, { job_id: id })])) return false;
    edit("");
    stage([]);
    snack("已交给 AI 整理");
    return true;
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;
    if (ai) return void organize();
    const c = checkTitle(title);
    if (!c.ok) return snack([...title.trim()].length > 200 && on === undefined ? `${c.error}；长段文字可以点 ✦ 交给 AI 整理` : c.error);
    if (!canSend()) return;
    const id = uuidv7();
    const op: Op = {
      op: "task.create",
      id,
      title: c.value,
      sort_key: lastKey(board.contents(into).map((t) => t.sort_key)),
      ...(into && { folder_id: into }),
      ...(on && { due: due(on) }),
    };
    if (!commit([op])) return;
    // Each file in a change of its own: one the server turns down leaves the task and the others.
    for (const f of staged) commit(addOps([f], { task_id: id }));
    edit("");
    stage([]);
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
      <form className="quick-add" hidden={hide} onSubmit={submit} onDragOver={files.onDragOver} onDrop={files.onDrop}>
        <div className="qa-field">
          <StagedFiles files={staged} unseen={hidden} onRemove={unstage} />
          {/* A title pasted on many lines is joined into one. */}
          <textarea
            onPaste={files.onPaste}
            ref={area}
            rows={1}
            value={title}
            onChange={(e) => edit(ai ? e.target.value : e.target.value.replace(/[\r\n]/g, ""))}
            onKeyDown={enter}
            placeholder={when ? `添加到${/^\d/.test(when) ? " " : ""}${when}` : ai ? "交给 AI 整理" : "添加事项"}
            autoComplete="off"
            enterKeyHint={ai ? undefined : "done"}
            aria-label={ai ? "交给 AI 整理的内容" : when ? `新事项标题，截止${when}` : "新事项标题"}
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
            {on === undefined && (
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
            )}
            {pick}
            {send}
          </div>
        </div>
      </form>
      {expanded && (
        <Editor
          text={title}
          staged={staged}
          unseen={hidden}
          files={files}
          onEdit={edit}
          onUnstage={unstage}
          onEnter={enter}
          onSubmit={() => organize() && setExpanded(false)}
          onClose={() => setExpanded(false)}
        >
          {pick}
          {send}
        </Editor>
      )}
      {picking && <FolderPicker mode="add" current={into} onPick={setTarget} onClose={() => setPicking(false)} />}
    </>
  );
}

/** AI organizing's text on a page of its own, to write at length: full screen on a phone, a large
 * window on a computer. Kept outside the bar's form, which would otherwise get its submits too. */
function Editor(props: {
  text: string;
  staged: Staged[];
  unseen?: Map<string, string>;
  /** What pasting or dropping files does. */
  files: { onPaste(e: React.ClipboardEvent): void; onDragOver(e: React.DragEvent): void; onDrop(e: React.DragEvent): void };
  onEdit(text: string): void;
  onUnstage(f: Staged): void;
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
            <StagedFiles files={props.staged} unseen={props.unseen} onRemove={props.onUnstage} />
            <textarea
              ref={area}
              value={props.text}
              onChange={(e) => props.onEdit(e.target.value)}
              onKeyDown={props.onEnter}
              onPaste={props.files.onPaste}
              onDragOver={props.files.onDragOver}
              onDrop={props.files.onDrop}
              placeholder="交给 AI 整理"
              aria-label="交给 AI 整理的内容"
            />
            <div className="editor-foot">{props.children}</div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
