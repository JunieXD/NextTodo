// What can be done to a folder: make one inside it, tidy it with AI, share, rename, move, delete. From
// the buttons on its page, or by right-clicking it on a computer.

import { Copy, FolderInput, FolderPlus, ListChecks, NotebookPen, Palette, Pencil, Trash2, Users, WandSparkles } from "lucide-react";
import { type ReactElement, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation } from "wouter";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import { aiReady } from "../ai";
import { MAX_OPS, lastKey } from "../domain/apply";
import { copyFolder, copyName, copyPlace } from "../domain/copy";
import { inLocalMode } from "../local";
import { localTime } from "../domain/dates";
import { rekey } from "../domain/reorder";
import { checkFolderName, checkJobText } from "../domain/text";
import { useCommit, useSession, useUndo, useView } from "../session";
import { dropFiles } from "../sync/files";
import { Modal, Tip, forUser, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { jobReminders } from "../remind-settings";
import { isSendKey, useEnterSends } from "../ui/keys";
import { ContextMenu, type MenuItem, MenuButton } from "../ui/menu";
import { useAiNote } from "./AiNotes";
import { useBatch } from "./batch";
import { FolderLookDialog } from "./folder-look";
import { folderUrl, settingsUrl } from "./nav";
import { FolderPicker } from "./pickers";
import { SharePanel, lost, shareable, sharer } from "./share";

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

/** Asks for a name and copies `f` with all in it, beside it (docs/design.md#文件夹). */
function useCopyFolder() {
  const { prompt, snack } = useFeedback();
  const commit = useCommit();
  const undo = useUndo();
  const { user, store, files } = useSession();
  return async (f: Folder) => {
    const parent = copyPlace(store.getView().board, f.id);
    const taken = (n: string) => store.getView().board.subfolders(parent).some((s) => s.name === n);
    const name = await prompt({
      title: `复制“${f.name}”`,
      label: "名称",
      value: copyName(f.name, taken),
      hint: "连同子文件夹、事项、附件和先后关系一起复制，事项均恢复为待办",
      ok: "复制",
      check: checkFolderName,
    });
    // As it is now: it may have changed, or moved, while the name was asked for.
    const board = store.getView().board;
    if (name === null || !board.folder(f.id)) return;
    const place = copyPlace(board, f.id);
    // Right after it; or, shared with the user, last at their root.
    const id = uuidv7();
    const wanted: { id: string; sort_key: string }[] = [...board.subfolders(place)];
    const at = wanted.findIndex((s) => s.id === f.id);
    wanted.splice(at < 0 ? wanted.length : at + 1, 0, { id, sort_key: "" });
    const keys = rekey(wanted, id);
    const copied = copyFolder(board, f.id, place, { id, name, sort_key: keys.find((k) => k.id === id)!.sort_key });
    const ops: Op[] = [...keys.filter((k) => k.id !== id).map((k): Op => ({ op: "folder.update", ...k })), ...copied.ops];
    if (ops.length > MAX_OPS) return snack(`文件夹内容过多，一次最多复制 ${MAX_OPS} 项（文件夹、事项、依赖和附件合计）`);
    const copies = copied.files.map(([copy, a]): [string, string, MutationId | undefined] => [copy, a.id, store.madeBy(a.id)]);
    try {
      // Noted first: the change waits for them as soon as it is made.
      await files.keepCopies(copies);
    } catch (e) {
      return snack(forUser("复制文件夹", e));
    }
    const r = commit(ops);
    if (!r) return void dropFiles(user.id, copies.map(([copy]) => copy));
    snack({ text: `已复制为“${name}”`, action: "撤销", onAction: () => undo(r.id) });
  };
}

/** Puts `folder` in `parent`, before the folder `before` there, or last; moved into another folder, says
 * so, with the undo. */
export function usePlaceFolder() {
  const { store } = useSession();
  const commit = useCommit();
  const undo = useUndo();
  const { snack } = useFeedback();
  return (folder: FolderId, parent: FolderId | null, before: FolderId | null) => {
    const board = store.getView().board;
    const f = board.folder(folder)!;
    const there = board.subfolders(parent);
    const wanted = there.filter((s) => s.id !== folder);
    const at = before ? wanted.findIndex((s) => s.id === before) : wanted.length;
    if (there[at]?.id === folder) return;
    wanted.splice(at, 0, f);
    const keys = rekey(wanted, folder);
    const sort_key = keys.find((k) => k.id === folder)?.sort_key;
    const moved = parent !== f.parent_id;
    // Moved, it may keep the key it had: last in a folder, say, as it was last in another.
    const ops: Op[] = [
      ...keys.filter((k) => k.id !== folder).map((k): Op => ({ op: "folder.update", ...k })),
      { op: "folder.update", id: folder, ...(sort_key && { sort_key }), ...(moved && { parent_id: parent }) },
    ];
    const r = commit(ops);
    if (r && moved) snack({ text: `已移至“${board.folderName(parent)}”${lost(r.effects)}`, action: "撤销", onAction: () => undo(r.id) });
  };
}

/** What a tidying asks for unless told otherwise. */
const TIDY = "把标题改清楚，描述改成简洁的书面语，补上漏掉的先后关系。";

/** Asks what to do with the tasks directly in `folder`, or the looks of it and the folders in it, and
 * hands that to AI, which suggests it (docs/ai.md#整理文件夹); `panel` is where it asks. */
function useTidy(folder: FolderId | null): { start(): void; panel: ReactNode } {
  const { snack } = useFeedback();
  const commit = useCommit();
  const { user, store } = useSession();
  const [, navigate] = useLocation();
  // The request it starts with, the usual one where there are tasks.
  const [asking, setAsking] = useState<string | null>(null);
  const start = async () => {
    const board = store.getView().board;
    const tasks = board.contents(folder).length > 0;
    // Nor a folder whose look it could change: the root has none, and only its owner restyles a shared one.
    if (!tasks && !board.subfolders(folder).length && (folder === null || sharer(board, folder))) return snack("这里没有事项");
    if (!(await aiReady(user.id))) {
      snack("先选好 AI 整理用的模型");
      return navigate(settingsUrl("ai"));
    }
    setAsking(tasks ? TIDY : "");
  };
  const submit = (text: string) => {
    const reminders = jobReminders(store.getView().board);
    const op = {
      op: "ai_job.create",
      id: uuidv7(),
      kind: "tidy",
      text,
      local_time: localTime(new Date()),
      ...(folder && { folder_id: folder }),
      ...(reminders && { reminders }),
    } as const;
    if (commit([op])) snack("已提交整理，完成后在列表顶部确认建议");
  };
  return { start, panel: asking !== null && <TidyRequest initial={asking} onSubmit={submit} onClose={() => setAsking(null)} /> };
}

/** The request for a tidying, in a box of many lines, sent by the keys set on the device. */
function TidyRequest({ initial, onSubmit, onClose }: { initial: string; onSubmit(text: string): void; onClose(): void }) {
  const [text, setText] = useState(initial);
  const [enterSends] = useEnterSends();
  const [error, setError] = useState("");
  const area = useRef<HTMLTextAreaElement>(null);
  // At the end of what is there, to add to it.
  useEffect(() => area.current!.setSelectionRange(initial.length, initial.length), [initial]);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const c = checkJobText(text);
    if (!c.ok) return setError(c.error);
    if (!c.value.trim()) return setError("要求不能为空");
    onSubmit(c.value);
    onClose();
  };
  const form = useId();
  return (
    <Modal
      title="AI 整理事项"
      onClose={onClose}
      focus={area}
      footer={
        <>
          <button type="button" className="text-btn" onClick={onClose}>
            取消
          </button>
          <button className="filled-btn" form={form}>
            开始整理
          </button>
        </>
      }
    >
      <form id={form} onSubmit={submit}>
        <label className={`field${error ? " error" : ""}`}>
          要求
          <textarea
            ref={area}
            className="job-text"
            rows={5}
            value={text}
            aria-invalid={error ? true : undefined}
            onChange={(e) => (setText(e.target.value), setError(""))}
            onKeyDown={(e) => {
              if (isSendKey(e, enterSends)) {
                e.preventDefault();
                e.currentTarget.form!.requestSubmit();
              }
            }}
          />
          {error ? (
            <span className="hint error-text" role="alert">
              {error}
            </span>
          ) : (
            <span className="hint">可新建、修改、完成、删除、移动事项，调整先后和顺序，设置文件夹的颜色和图标，执行前逐条确认</span>
          )}
        </label>
      </form>
    </Modal>
  );
}

type FolderActions = { create: MenuItem; tidy: MenuItem; note: MenuItem; rest: MenuItem[]; panel: ReactNode };

/** The actions on `folder`, and the panel that moving, tidying or sharing it opens: making a folder in
 * it, the two of AI, and the rest; in the root, no more; in a folder someone shares with the user, not
 * changing that folder itself, which is its owner's to do. None for a folder deleted meanwhile, say on
 * another device. */
function useFolderActions(folder: FolderId | null): FolderActions | null {
  const commit = useCommit();
  const undo = useUndo();
  const { prompt, confirm, snack } = useFeedback();
  const newFolder = useNewFolder();
  const copy = useCopyFolder();
  const place = usePlaceFolder();
  const tidy = useTidy(folder);
  const note = useAiNote(folder);
  const [location, navigate] = useLocation();
  const [moving, setMoving] = useState(false);
  const [sharing, setSharing] = useState(false);
  const [styling, setStyling] = useState(false);
  const { board } = useView();
  const ai = {
    tidy: { icon: WandSparkles, label: "AI 整理事项", onSelect: tidy.start },
    note: { icon: NotebookPen, label: "AI 说明", onSelect: note.start },
  };
  if (folder === null) {
    const panel = (
      <>
        {tidy.panel}
        {note.panel}
      </>
    );
    return { create: { icon: FolderPlus, label: "新建文件夹", onSelect: () => newFolder(null) }, ...ai, rest: [], panel };
  }
  const f = board.folder(folder);
  if (!f) return null;

  const rename = async () => {
    const name = await prompt({ title: "重命名文件夹", label: "名称", value: f.name, ok: "保存", check: checkFolderName });
    if (name !== null && name !== f.name) commit([{ op: "folder.update", id: f.id, name }]);
  };
  const remove = async () => {
    const inside = [...board.snapshot.tasks].filter((t) => t.folder_id && board.inSubtree(t.folder_id, f.id)).length;
    const body = inside ? `其中的子文件夹和 ${inside} 个事项将一并删除，可撤销。` : "其中的子文件夹将一并删除，可撤销。";
    if (!(await confirm({ title: `删除“${f.name}”？`, body, ok: "删除", danger: true }))) return;
    const r = commit([{ op: "folder.delete", id: f.id }]);
    if (!r) return;
    // Out of a page that has gone with it.
    const shown = location.match(/^\/folders\/([^/]+)$/)?.[1];
    if (shown && board.inSubtree(shown, f.id)) navigate(folderUrl(f.parent_id), { replace: true });
    snack({ text: `已删除“${f.name}”`, action: "撤销", onAction: () => undo(r.id) });
  };

  const theirs = sharer(board, f.id);
  const copying = { icon: Copy, label: "复制…", onSelect: () => void copy(f) };
  return {
    create: { icon: FolderPlus, label: "新建子文件夹", onSelect: () => newFolder(f.id) },
    ...ai,
    rest: [
      ...(shareable(board, f.id) && !inLocalMode() ? [{ icon: Users, label: "共享…", onSelect: () => setSharing(true) }] : []),
      ...(theirs
        ? [copying]
        : [
            { icon: Pencil, label: "重命名", onSelect: rename },
            { icon: Palette, label: "颜色和图标…", onSelect: () => setStyling(true) },
            copying,
            { icon: FolderInput, label: "移动到…", onSelect: () => setMoving(true) },
            { icon: Trash2, label: "删除文件夹", danger: true, onSelect: remove },
          ]),
    ],
    panel: (
      <>
        {tidy.panel}
        {note.panel}
        {moving && <FolderPicker mode="move" current={f.parent_id} exclude={f.id} onPick={(parent) => place(f.id, parent, null)} onClose={() => setMoving(false)} />}
        {sharing && <SharePanel folder={f.id} onClose={() => setSharing(false)} />}
        {styling && <FolderLookDialog board={board} folder={f} onClose={() => setStyling(false)} />}
      </>
    ),
  };
}

/** The buttons of the folder shown, or of the root: tidying it, often wanted, on its own, and "…" with
 * the rest of what its right-click menu has and choosing its tasks for
 * [batch operations](docs/ui/batch.md). */
export function FolderMenu({ folder }: { folder: FolderId | null }) {
  const actions = useFolderActions(folder);
  const { snack } = useFeedback();
  const { board } = useView();
  const batch = useBatch();
  if (!actions) return null;
  const { create, tidy, note, rest, panel } = actions;
  const choose = () => (board.contents(folder).length ? batch.start() : snack("这里没有事项"));
  return (
    <>
      <Tip label={tidy.label}>
        <button className="icon-btn" onClick={tidy.onSelect}>
          <Icon of={tidy.icon} />
        </button>
      </Tip>
      <MenuButton label="文件夹操作" items={[create, note, { icon: ListChecks, label: "批量操作", onSelect: choose }, ...rest]} />
      {panel}
    </>
  );
}

/** Right-clicking `children`, a row standing for the folder (`null` for the root), opens its menu. */
export function FolderContextMenu({ folder, children }: { folder: FolderId | null; children: ReactElement }) {
  const actions = useFolderActions(folder);
  const items = actions ? [actions.create, actions.tidy, actions.note, ...actions.rest] : [];
  return (
    <>
      <ContextMenu items={items}>{children}</ContextMenu>
      {actions?.panel}
    </>
  );
}
