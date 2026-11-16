// Notes for AI organizing: what it keeps to, folder by folder from the root down (docs/ai.md#说明).

import { ChevronRight, Plus } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api/http";
import type { AiNote } from "../api/types/AiNote";
import type { FolderId } from "../api/types/FolderId";
import type { Board } from "../domain/board";
import { checkNoteText } from "../domain/text";
import { useView } from "../session";
import { Modal, forUser, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { FolderPicker } from "./pickers";

const noteOf = (notes: AiNote[], f: FolderId | null) => notes.find((n) => n.folder_id === f)?.text;

const place = (board: Board, f: FolderId | null) => (f === null ? "全局" : board.folderPath(f));

/** The folders above `f`, outermost first: the root, then down to its parent. */
function above(board: Board, f: FolderId | null): (FolderId | null)[] {
  const up: (FolderId | null)[] = [];
  for (let cur = f; cur !== null; ) {
    cur = board.folder(cur)?.parent_id ?? null;
    up.unshift(cur);
  }
  return up;
}

/** The user's notes as the server has them, once `load` has fetched them; it says whether it could. */
function useNotes() {
  const { snack } = useFeedback();
  const [notes, setNotes] = useState<AiNote[]>();
  const load = useCallback(
    () =>
      api<AiNote[]>("GET", "/ai/notes").then(
        (n) => (setNotes(n), true),
        (e) => (snack(forUser("读取 AI 说明", e)), false),
      ),
    [snack],
  );
  return { notes, load };
}

/** Writes the note for `folder`, showing first those above it that come with it. */
function NoteEditor({ folder, notes, onSaved, onClose }: { folder: FolderId | null; notes: AiNote[]; onSaved?(): void; onClose(): void }) {
  const { board } = useView();
  const { snack } = useFeedback();
  const was = noteOf(notes, folder);
  const [text, setText] = useState(was ?? "");
  const [error, setError] = useState("");
  const area = useRef<HTMLTextAreaElement>(null);
  const inherited = above(board, folder).flatMap((f) => {
    const t = noteOf(notes, f);
    return t === undefined ? [] : [{ f, t }];
  });
  const save = async (to: string) => {
    const c = checkNoteText(to);
    if (!c.ok) return setError(c.error);
    try {
      await api("PUT", "/ai/notes", { folder_id: folder, text: c.value } satisfies AiNote);
      onSaved?.();
      onClose();
    } catch (e) {
      snack(forUser(c.value ? "保存 AI 说明" : "清除 AI 说明", e));
    }
  };
  return (
    <Modal title="AI 说明" onClose={onClose} focus={area}>
      <p className="note-place">{place(board, folder)}</p>
      {inherited.length > 0 && (
        <div className="note-above">
          也会用到
          {inherited.map(({ f, t }) => (
            <p key={f ?? "root"}>
              <b>{place(board, f)}</b>
              {t}
            </p>
          ))}
        </div>
      )}
      <form onSubmit={(e) => (e.preventDefault(), void save(text))}>
        <label className={`field${error ? " error" : ""}`}>
          <textarea
            ref={area}
            rows={6}
            value={text}
            aria-label="说明"
            aria-invalid={error ? true : undefined}
            placeholder="例如：标题以模块名开头，如“登录：修复超时”"
            onChange={(e) => (setText(e.target.value), setError(""))}
          />
          {error && (
            <span className="hint error-text" role="alert">
              {error}
            </span>
          )}
        </label>
        <div className="actions">
          {was !== undefined && (
            <button type="button" className="text-btn danger-text" onClick={() => void save("")}>
              清除说明
            </button>
          )}
          <button className="filled-btn">保存</button>
        </div>
      </form>
    </Modal>
  );
}

/** The notes on the AI settings page: the global one, those of folders, and a row to add one. */
export function AiNotes() {
  const { board } = useView();
  const { notes, load } = useNotes();
  const [editing, setEditing] = useState<FolderId | null>();
  const [picking, setPicking] = useState(false);
  useEffect(() => void load(), [load]);
  if (!notes) return null;
  const row = (f: FolderId | null) => {
    const t = noteOf(notes, f);
    return (
      <button key={f ?? "root"} className="row" onClick={() => setEditing(f)}>
        <div className="row-inner">
          <span className="folder-main">
            <span>{place(board, f)}</span>
            <span className="folder-sub">{t ?? "未设置"}</span>
          </span>
          <span className="chev">
            <Icon of={ChevronRight} size={18} />
          </span>
        </div>
      </button>
    );
  };
  return (
    <>
      <h2 className="group-title">说明</h2>
      <p className="empty">AI 整理时照着做，比如“标题以模块名开头”。文件夹的说明对它的子文件夹也有效，全局的对所有文件夹都有效。</p>
      <div className="group settings">
        {row(null)}
        {board
          .folderTree()
          .filter((f) => f !== null && noteOf(notes, f) !== undefined)
          .map(row)}
        <button className="row" onClick={() => setPicking(true)}>
          <div className="row-inner">
            <Icon of={Plus} size={18} />
            为文件夹添加说明
          </div>
        </button>
      </div>
      {picking && <FolderPicker mode="note" onPick={setEditing} onClose={() => setPicking(false)} />}
      {/* Not for a folder gone meanwhile: deleted, or no longer shared with the user. */}
      {editing !== undefined && board.hasFolder(editing) && <NoteEditor folder={editing} notes={notes} onSaved={() => void load()} onClose={() => setEditing(undefined)} />}
    </>
  );
}

/** The note for `folder`, from its menu; `panel` is where it is written. */
export function useAiNote(folder: FolderId | null): { start(): void; panel: ReactNode } {
  const { notes, load } = useNotes();
  const [open, setOpen] = useState(false);
  const start = () => void load().then(setOpen);
  return { start, panel: open && notes && <NoteEditor folder={folder} notes={notes} onClose={() => setOpen(false)} /> };
}
