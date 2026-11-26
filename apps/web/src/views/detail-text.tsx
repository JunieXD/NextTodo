// A task's title and description edited in its detail, and what to keep when changed elsewhere meanwhile.

import { Suspense, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Attachment } from "../api/types/Attachment";
import type { MutationId } from "../api/types/MutationId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { filesIn, reference } from "../attach";
import { checkDescription, checkTitle } from "../domain/text";
import { useSession, useUndo } from "../session";
import { Seg } from "../ui/controls";
import { useFeedback } from "../ui/feedback";
import { composing } from "../ui/keys";
import { Description } from "../ui/lazy-markdown";
import { ListButtons } from "../ui/md-lists";
import type { Adding } from "./attachments";

/** Who made a change: the user's own token, or someone else, by name or by their token, `Bob 的
 * Claude Code`; nobody named for the user themselves. */
export function source(board: Board, user: string, via: string | null): string | null {
  if (user === board.me) return via;
  return via ? `${board.name(user)} 的 ${via}` : board.name(user);
}

/** Saves a text field; what `useCommit` gives back. */
type Save = (value: string) => { id: MutationId } | null;

/** An edit in progress and the text it started from. */
type Draft = { text: string; base: string };

/** What an edit left in its field, and whether that is the edit. */
type Kept = { value: string; mine: boolean };

/** Saves `mine`, edited from `base`, into a field of a task. When the field has changed elsewhere
 * meanwhile, asks whether to keep that or overwrite it. Overwriting can be undone, while giving up the
 * edit cannot, so closing the dialog overwrites; it overwrites whatever is there by the answer, which
 * undoing brings back. Gives what the field holds afterwards; nothing when the task is gone. */
function useKeepEdit(field: "title" | "description", what: string) {
  const { store } = useSession();
  const { confirm, snack } = useFeedback();
  const undo = useUndo();
  return async (id: TaskId, base: string, mine: string, save: Save): Promise<Kept | null> => {
    const current = () => store.getView().board.task(id);
    let t = current();
    if (!t) return null;
    // Nothing edited: what is there stays, whoever wrote it.
    if (mine === base) return { value: t[field], mine: false };
    if (t[field] !== base && t[field] !== mine) {
      const by = source(store.getView().board, t.updated_by, t.updated_via);
      const theirs = await confirm({
        title: `${what}在编辑期间已${by ? `被“${by}”` : "在别处"}改过`,
        body: "对方现在的版本如下。保留对方的版本，本次修改将丢弃。",
        detail: t[field],
        ok: "保留对方的",
        cancel: "用我的覆盖",
        danger: true,
      });
      t = current();
      if (!t) return null;
      if (theirs) return { value: t[field], mine: false };
      if (t[field] !== mine) {
        const r = save(mine);
        if (r) snack({ text: `已用本次修改覆盖${what}`, action: "撤销", onAction: () => undo(r.id) });
      }
      return { value: mine, mine: true };
    }
    if (t[field] !== mine) save(mine);
    return { value: mine, mine: true };
  };
}

/** The title, edited in place. */
export function Title({ task, onSave }: { task: Task; onSave: Save }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const { snack } = useFeedback();
  const keep = useKeepEdit("title", "标题");
  // Where the cursor starts in the box: where the title was clicked.
  const caret = useRef(0);
  const placeCaret = useCallback((box: HTMLTextAreaElement | null) => box?.setSelectionRange(caret.current, caret.current), []);
  // Kept as soon as it is rendered: the details can close right after a blur, in the same go.
  const pending = useRef({ draft, task, onSave });
  useLayoutEffect(() => {
    pending.current = { draft, task, onSave };
  });
  /** Saves the edit, taking it out, so that leaving right after a blur does not save it again. */
  const finish = () => {
    const { draft: d, task: t, onSave: done } = pending.current;
    if (!d) return;
    pending.current.draft = null;
    const c = checkTitle(d.text);
    if (!c.ok) snack(c.error);
    else void keep(t.id, d.base, c.value, done);
  };
  // Closing the details by a swipe or the back button takes the box away without it losing focus.
  // eslint-disable-next-line react-hooks/exhaustive-deps -- saves once, when leaving
  useEffect(() => finish, []);
  if (draft === null) {
    return (
      <h2 className="d-title">
        <button
          type="button"
          onClick={(e) => {
            // From the keyboard (no `detail`), at the end.
            const at = e.detail ? caretAt(e.clientX, e.clientY) : null;
            caret.current = at && at[0] === e.currentTarget.firstChild ? at[1] : task.title.length;
            setDraft({ text: task.title, base: task.title });
          }}
        >
          {task.title}
        </button>
      </h2>
    );
  }
  return (
    <textarea
      ref={placeCaret}
      className="title-edit"
      autoFocus
      rows={1}
      value={draft.text}
      aria-label="标题"
      onChange={(e) => setDraft({ ...draft, text: e.target.value.replace(/\n/g, "") })}
      onBlur={() => {
        finish();
        setDraft(null);
      }}
      onKeyDown={(e) => {
        if (composing(e)) return;
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          pending.current.draft = null;
          setDraft(null);
        }
      }}
    />
  );
}

/** The text node and offset in it where a click at (`x`, `y`) puts the cursor. */
function caretAt(x: number, y: number): [Node, number] | null {
  // The standard one is new; older WebViews have only WebKit's own.
  if (typeof document.caretPositionFromPoint === "function") {
    const p = document.caretPositionFromPoint(x, y);
    return p && [p.offsetNode, p.offset];
  }
  const r = document.caretRangeFromPoint(x, y);
  return r && [r.startContainer, r.startOffset];
}

/** The description: shown as Markdown with tickable checklists and the task's attachments referred to,
 * or edited as text, where pasting or dropping files adds them and refers to them. An edit is saved
 * when leaving the editor, also by switching to another task. While it is edited, `onEditing` is given
 * what puts text where the cursor is. */
export function DescriptionSection({
  task,
  attachments,
  onEditing,
  onSave,
  onFiles,
  onAttachment,
}: {
  task: Task;
  attachments: Attachment[];
  onEditing(insert: ((text: string) => void) | null): void;
  onSave: Save;
  onFiles(files: File[]): Adding;
  onAttachment(a: Attachment): void;
}) {
  const [draft, setDraft] = useState<Draft | null>(null);
  // How many times another's description was taken instead of the draft: undo starts again from it.
  const [taken, setTaken] = useState(0);
  const area = useRef<HTMLTextAreaElement>(null);
  const { snack } = useFeedback();
  const keep = useKeepEdit("description", "描述");
  const pending = useRef({ draft, task, onSave });
  useEffect(() => {
    pending.current = { draft, task, onSave };
  });
  /** The save waiting for an answer, if any: leaving meanwhile does not save, or ask, a second time. */
  const saving = useRef<Promise<Kept | null>>(undefined);
  /** Saves the draft; gives what the description holds afterwards, or nothing when it was not saved. */
  const save = (d: Draft | null, t: Task, done: Save) => {
    if (d === null || saving.current) return saving.current;
    const c = checkDescription(d.text);
    if (!c.ok) return void snack(c.error);
    saving.current = keep(t.id, d.base, c.value, done).finally(() => (saving.current = undefined));
    return saving.current;
  };
  useEffect(
    () => () => {
      const { draft, task, onSave } = pending.current;
      void save(draft, task, onSave);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps -- saves once, when leaving
    [],
  );
  /** Puts `text` in place of what is from `from` to `to` in the editor, the cursor after it. */
  const insert = (text: string, [from, to]: [number, number]) => {
    setDraft((d) => d && { ...d, text: d.text.slice(0, from) + text + d.text.slice(to) });
    requestAnimationFrame(() => area.current?.setSelectionRange(from + text.length, from + text.length));
  };
  useEffect(() => {
    if (draft === null) return;
    onEditing((text) => insert(text, [area.current!.selectionStart, area.current!.selectionEnd]));
    return () => onEditing(null);
  });
  /** Adds `files` and refers to them in place of what is selected; takes back the references to those
   * that could not be added, while still editing and as put. */
  const addHere = (files: File[], at: [number, number]) => {
    const { planned, added } = onFiles(files);
    if (!planned.length) return;
    insert(planned.map(reference).join(" "), at);
    void added.then((ids) => {
      const failed = planned.filter((p) => !ids.includes(p.id));
      if (failed.length) setDraft((d) => d && { ...d, text: failed.reduce((text, p) => text.replace(reference(p), ""), d.text) });
    });
  };
  const mode = (v: "view" | "edit") => {
    if (v === "edit") setDraft({ text: task.description, base: task.description });
    else {
      void save(draft, task, onSave);
      setDraft(null);
    }
  };
  /** Saves on leaving the box while still editing: what is saved, or kept instead, is the base from then on. */
  const blur = async () => {
    const kept = await save(draft, task, onSave);
    if (!kept) return;
    if (kept.mine) setDraft((d) => d && { ...d, base: kept.value });
    else {
      if (kept.value !== area.current?.value) setTaken((n) => n + 1);
      setDraft((d) => d && { text: kept.value, base: kept.value });
    }
  };
  return (
    <>
      <div className="d-sec">
        <h3>描述</h3>
        <Seg
          label="描述"
          small
          items={[
            ["view", "预览"],
            ["edit", "编辑"],
          ]}
          value={draft === null ? "view" : "edit"}
          onChange={mode}
        />
      </div>
      <div className={draft === null ? "group" : "group writing"}>
        {draft === null ? (
          <div className="md">
            <Suspense>
              <Description source={task.description} attachments={attachments} onChange={onSave} onAttachment={onAttachment} />
            </Suspense>
          </div>
        ) : (
          <textarea
            ref={area}
            className="md-edit"
            value={draft.text}
            placeholder="支持 Markdown，以 - [ ] 开头为清单项"
            onChange={(e) => setDraft({ ...draft, text: e.target.value })}
            onBlur={() => void blur()}
            onPaste={(e) => {
              const files = filesIn(e.clipboardData);
              if (!files.length) return;
              e.preventDefault();
              addHere(files, [e.currentTarget.selectionStart, e.currentTarget.selectionEnd]);
            }}
            onDrop={(e) => {
              const files = filesIn(e.dataTransfer);
              if (!files.length) return;
              e.preventDefault();
              addHere(files, [e.currentTarget.selectionStart, e.currentTarget.selectionStart]);
            }}
          />
        )}
        {draft !== null && <ListButtons key={taken} area={area} />}
      </div>
    </>
  );
}
