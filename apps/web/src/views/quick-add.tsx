// The quick-add bar: adds tasks to a folder or hands content to AI organizing, with attachments if
// any. In the calendar, new tasks are due on the day picked, at its time if it has one.

import { ArrowUp, ChevronDown, ChevronUp, Folder as FolderIcon, Paperclip, Sparkles } from "lucide-react";
import { Dialog } from "radix-ui";
import { useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation, useSearchParams } from "wouter";
import { NextTodo, inCard } from "../mobile";
import { app } from "../api/http";
import type { AiJobId } from "../api/types/AiJobId";
import type { Due } from "../api/types/Due";
import type { FolderId } from "../api/types/FolderId";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { TaskId } from "../api/types/TaskId";
import { lastKey } from "../domain/apply";
import { aiReady, setAiMode, useAiMode, useAiVision } from "../ai";
import { day, formatDay, inOrder, localTime } from "../domain/dates";
import { blank, checkJobText, checkTitle } from "../domain/text";
import { useCommit, useConnection, useSession, useView } from "../session";
import { useNextFolders } from "../next-folders";
import { defaultReminders, jobReminders } from "../remind-settings";
import { addOps, draggingFiles, filesIn, isPicture, plan, seesAny, unseen } from "../attach";
import { type Staged, draft, keepDraft, keepStaged, onDrafts, stagedFiles } from "../sync/drafts";
import { dropFiles } from "../sync/files";
import { useBack } from "../ui/back";
import { RootContext, Tip, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { composing, isSendKey, useEnterSends } from "../ui/keys";
import { useLeave } from "../ui/leave";
import { addedTo, pickAddTo, useAddTo } from "./add-to";
import { ChooseFiles, StagedFiles, useKeepFiles } from "./attachments";
import { settingsUrl } from "./nav";
import { FolderPicker } from "./pickers";

/** A due date on `date`, at `time` if any, with this device's defaults for one so. */
function due(date: string, time: string | null): Due {
  const defaults = defaultReminders();
  if (time === null) {
    const reminders = inOrder(defaults.dated, undefined);
    return { date, ...(reminders.length > 0 && { reminders }) };
  }
  const reminders = inOrder(defaults.timed, time);
  return { date, time, ...(reminders.length > 0 && { reminders }) };
}

/** What a quick add sent: its changes, and the task it added or the AI job it asked for. */
export type Sent = { changes: MutationId[] } & ({ task: TaskId; job?: never } | { job: AiJobId; task?: never });

/** Where the quick add bar keeps its draft and files: one in the calendar, one in each folder, and one
 * for what is next, which adds to a folder picked beside it. */
export const quickAddPlace = (folder?: FolderId | null, on?: string) => (on !== undefined ? "calendar" : folder === undefined ? "next" : (folder ?? "root"));

/** Files dragged in anywhere on the page but on the details, which take them for their task, for `add`
 * as they are dropped; while they are dragged there, `on`. None while `off`. */
function useDropAnywhere(add: (files: File[]) => unknown, off: boolean) {
  const [on, setOn] = useState(false);
  const latest = useRef(add);
  useEffect(() => {
    latest.current = add;
  });
  useEffect(() => {
    if (off) return;
    const ours = (e: DragEvent) => draggingFiles(e) && !(e.target instanceof Element && e.target.closest(".detail"));
    // Entering an element leaves the one before; out of the page once as many as went in are left.
    let depth = 0;
    const enter = (e: DragEvent) => draggingFiles(e) && depth++;
    const leave = (e: DragEvent) => draggingFiles(e) && --depth === 0 && setOn(false);
    const over = (e: DragEvent) => {
      setOn(ours(e));
      if (ours(e)) e.preventDefault();
    };
    const drop = (e: DragEvent) => {
      depth = 0;
      setOn(false);
      if (!ours(e)) return;
      e.preventDefault();
      void latest.current(filesIn(e.dataTransfer));
    };
    document.addEventListener("dragenter", enter);
    document.addEventListener("dragleave", leave);
    document.addEventListener("dragover", over);
    document.addEventListener("drop", drop);
    return () => {
      document.removeEventListener("dragenter", enter);
      document.removeEventListener("dragleave", leave);
      document.removeEventListener("dragover", over);
      document.removeEventListener("drop", drop);
      setOn(false);
    };
  }, [off]);
  return on;
}

/** Adds tasks by typing, or, switched to AI organizing with ✦, has AI organize what is written into
 * tasks; that text can be expanded to write at length. Files pasted or dropped in go with what is
 * added. In a folder (`null` for the root) it adds there; elsewhere to a folder picked beside it, as
 * `add-to.ts` keeps it. In the calendar, `on` the day picked there, a task is due that day, with the
 * defaults for a due date; AI organizing there also receives the day and time if picked. Hidden by
 * `hide`, it keeps what it has, the folder picked and the files still being saved. `onSent` hears of
 * what each add sent. `at` a time picked on the day, it is due then, and takes the focus. */
export function QuickAdd({
  folder,
  on,
  at = null,
  hide = false,
  onSent,
}: {
  folder?: FolderId | null;
  on?: string;
  at?: string | null;
  hide?: boolean;
  onSent?: (sent: Sent) => void;
}) {
  const { board } = useView();
  const commit = useCommit();
  const { snack } = useFeedback();
  const { user } = useSession();
  const place = quickAddPlace(folder, on);
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
  /** What pasting files does in the box's text. */
  const files = {
    onPaste: (e: React.ClipboardEvent) => {
      const list = filesIn(e.clipboardData);
      if (!list.length) return;
      e.preventDefault();
      void addFiles(list);
    },
  };
  const dropping = useDropAnywhere(addFiles, hide);
  /** Whether the files staged can go; one the server turned down has to be taken out first. */
  const canSend = () => {
    const refused = staged.find((f) => f.error);
    if (refused) snack(`“${refused.name}”上传失败，去掉后再提交`);
    return !refused;
  };
  // The device remembers the mode last used.
  const canOrganize = !inCard || on === undefined;
  const ai = useAiMode() && canOrganize;
  const [enterSends] = useEnterSends();
  const online = useConnection() === "online";
  const vision = useAiVision(user.id, online && ai && staged.some((f) => isPicture(f.type)));
  const hidden = ai && vision !== undefined ? unseen(staged, vision) : undefined;
  // Pictures the model sees say it all: AI organizing needs no text with them (docs/ai.md#附件).
  const empty = blank(title) && !(ai && vision !== undefined && seesAny(staged, vision));
  // Until one is picked or fixed, in “接下来” the first folder it lists, so that a new task shows
  // there; in the calendar, the root.
  const target = useAddTo(board);
  const listed = useNextFolders(board);
  const [picking, setPicking] = useState(false);
  const [tall, setTall] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const into =
    folder !== undefined
      ? folder
      : target !== undefined
        ? target
        : on === undefined
          ? (board.folderTree().find((f) => listed.includes(f)) ?? null)
          : null;
  const when = on && `${formatDay(day(on).getTime(), new Date())}${at ? ` ${at}` : ""}`;
  const area = useRef<HTMLTextAreaElement>(null);
  const focus = () => area.current?.focus();
  // A time picked on the day is to add to now.
  useEffect(() => {
    if (at !== null) area.current?.focus();
  }, [on, at]);
  // Narrower than this, the buttons beside the field would leave it a few characters: they go below
  // it, and only while it is in use.
  const form = useRef<HTMLFormElement>(null);
  const [narrow, setNarrow] = useState(false);
  useLayoutEffect(() => {
    const el = form.current!;
    const resized = new ResizeObserver(() => setNarrow(el.clientWidth < 480));
    resized.observe(el);
    return () => resized.disconnect();
  }, []);
  // In use from when the focus comes in, or a tap, until it or a tap goes elsewhere in the page: not to a
  // menu or a dialog the bar opened. Away from the page, to choose a file say, does not count. A tap
  // elsewhere may leave the focus in the field, and one on it again then brings no focus to take.
  const [active, setActive] = useState(false);
  useEffect(() => {
    if (!active) return;
    const away = (e: Event) =>
      e.target instanceof Element && !form.current!.contains(e.target) && !e.target.closest('[role="menu"], [role="dialog"]') && setActive(false);
    addEventListener("pointerdown", away, true);
    addEventListener("focusin", away, true);
    return () => {
      removeEventListener("pointerdown", away, true);
      removeEventListener("focusin", away, true);
    };
  }, [active]);
  // The quick add card is in use whenever it is up.
  const layered = narrow || inCard;
  const shut = narrow && !inCard && !active && !title && !staged.length;
  // Android's quick add card opens with `?add` to type a new task straight away, and so does the
  // desktop app's panel each time it opens: on Android with the keyboard up, which focusing alone does
  // not bring.
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
    const reminders = jobReminders();
    const op: Op = {
      op: "ai_job.create",
      id,
      text: c.value,
      local_time: localTime(new Date()),
      ...(into && { folder_id: into }),
      ...(reminders && { reminders }),
      ...(on && { day: on, ...(at && { time: at }) }),
    };
    const made = commit([op, ...addOps(staged, { job_id: id })]);
    if (!made) return false;
    edit("");
    stage([]);
    if (folder === undefined) addedTo(user.id, into);
    snack("已交给 AI 整理");
    onSent?.({ changes: [made.id], job: id });
    return true;
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (empty) return;
    if (ai) return void organize();
    const c = checkTitle(title);
    if (!c.ok) return snack([...title.trim()].length > 200 && canOrganize ? `${c.error}；长段文字可点 ✦ 交给 AI 整理` : c.error);
    if (!canSend()) return;
    const id = uuidv7();
    const op: Op = {
      op: "task.create",
      id,
      title: c.value,
      sort_key: lastKey(board.contents(into).map((t) => t.sort_key)),
      ...(into && { folder_id: into }),
      ...(on && { due: due(on, at) }),
    };
    const made = commit([op]);
    if (!made) return;
    const changes = [made.id];
    // Each file in a change of its own: one the server turns down leaves the task and the others.
    for (const f of staged) {
      const added = commit(addOps([f], { task_id: id }));
      if (added) changes.push(added.id);
    }
    edit("");
    stage([]);
    if (folder === undefined) addedTo(user.id, into);
    snack(`已添加“${c.value}”`);
    onSent?.({ changes, task: id });
  };

  // A title is one line, so Enter adds it; AI organizing's text has many, sent by the keys set on the
  // device. Not while an input method is still composing.
  const enter = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (ai ? isSendKey(e, enterSends) : e.key === "Enter" && !composing(e)) {
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
      <button type="submit" className="qa-btn icon send" disabled={empty}>
        <Icon of={ArrowUp} size={18} width={2.2} />
      </button>
    </Tip>
  );
  const clip = (
    <ChooseFiles className="qa-btn icon" label="添加附件" onAdd={(list) => void addFiles(list)}>
      <Icon of={Paperclip} size={16} />
    </ChooseFiles>
  );
  // Not taking the focus from the text: on a phone the keyboard would go down and come up again.
  const spark = canOrganize && (
    <Tip label="AI 整理">
      <button type="button" className={`qa-btn icon${ai ? " on" : ""}`} aria-pressed={ai} onMouseDown={(e) => e.preventDefault()} onClick={() => void switchMode()}>
        <Icon of={Sparkles} size={16} />
      </button>
    </Tip>
  );
  const expand = ai && tall && (
    <Tip label="展开编辑">
      <button type="button" className="qa-btn icon expand" onClick={() => setExpanded(true)}>
        <Icon of={ChevronUp} size={18} />
      </button>
    </Tip>
  );

  return (
    <>
      <form
        ref={form}
        className={`quick-add${layered ? " layered" : ""}${shut ? " shut" : ""}`}
        hidden={hide}
        onSubmit={submit}
        onFocus={() => setActive(true)}
        onPointerDown={() => setActive(true)}
      >
        {dropping && <div className="drop-cover">松开添加附件</div>}
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
        {layered ? (
          <div className="qa-row" inert={shut}>
            <div className="qa-row-in">
              <div className="qa-btns">
                {clip}
                {spark}
                {expand}
              </div>
              <div className="qa-btns">
                {pick}
                {send}
              </div>
            </div>
          </div>
        ) : (
          <div className="qa-side">
            {expand}
            <div className="qa-btns">
              {clip}
              {spark}
              {pick}
              {send}
            </div>
          </div>
        )}
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
          onSubmit={() => !empty && organize() && setExpanded(false)}
          onClose={() => setExpanded(false)}
          clip={clip}
        >
          {pick}
          {send}
        </Editor>
      )}
      {picking && <FolderPicker mode="add" current={into} onPick={(f) => pickAddTo(user.id, f)} onClose={() => setPicking(false)} />}
    </>
  );
}

/** AI organizing's text on a page of its own, to write at length: full screen on a phone, a large
 * window on a computer. Kept outside the bar's form, which would otherwise get its submits too. */
function Editor(props: {
  text: string;
  staged: Staged[];
  unseen?: Map<string, string>;
  /** What pasting files does. */
  files: { onPaste(e: React.ClipboardEvent): void };
  onEdit(text: string): void;
  onUnstage(f: Staged): void;
  onEnter(e: React.KeyboardEvent<HTMLTextAreaElement>): void;
  onSubmit(): void;
  onClose(): void;
  /** The button to add files, at the start of the foot. */
  clip: React.ReactNode;
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
              props.onSubmit();
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
              placeholder="交给 AI 整理"
              aria-label="交给 AI 整理的内容"
            />
            <div className="editor-foot">
              {props.clip}
              {props.children}
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
