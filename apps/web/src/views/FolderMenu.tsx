// What can be done to a folder: make one inside it, tidy it with AI, rename, move, delete. From its "…"
// button, or by right-clicking it on a computer.

import { DropdownMenu } from "radix-ui";
import { Ellipsis, FolderInput, FolderPlus, ListChecks, Pencil, Sparkles, Trash2 } from "lucide-react";
import { type ReactElement, type ReactNode, useContext, useEffect, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { aiReady } from "../ai";
import { lastKey } from "../domain/apply";
import { localTime } from "../domain/dates";
import { checkFolderName, checkJobText } from "../domain/text";
import { useCommit, useSession, useUndo, useView } from "../session";
import { useBack } from "../ui/back";
import { Modal, RootContext, Tip, useFeedback } from "../ui/feedback";
import { composing } from "../ui/keys";
import { Icon } from "../ui/icons";
import { ContextMenu, type MenuItem, itemClass } from "../ui/menu";
import { useBatch } from "./batch";
import { folderUrl, settingsUrl } from "./nav";
import { FolderPicker } from "./pickers";

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

/** What a tidying asks for unless told otherwise. */
const TIDY = "把标题改清楚，描述改成简洁的书面语，补上漏掉的先后关系。";

/** Asks what to do with the tasks directly in `folder` and hands them to AI, which suggests it
 * (docs/ai.md#整理文件夹); `panel` is where it asks. */
function useTidy(folder: FolderId | null): { start(): void; panel: ReactNode } {
  const { snack } = useFeedback();
  const commit = useCommit();
  const { user, store } = useSession();
  const [, navigate] = useLocation();
  const [asking, setAsking] = useState(false);
  const start = async () => {
    if (!store.getView().board.contents(folder).length) return snack("这里没有事项");
    if (!(await aiReady(user.id))) {
      snack("先选好 AI 整理用的模型");
      return navigate(settingsUrl("ai"));
    }
    setAsking(true);
  };
  const submit = (text: string) => {
    const op = { op: "ai_job.create", id: uuidv7(), kind: "tidy", text, local_time: localTime(new Date()), ...(folder && { folder_id: folder }) } as const;
    if (commit([op])) snack("已提交整理，完成后在列表顶部确认建议");
  };
  return { start, panel: asking && <TidyRequest onSubmit={submit} onClose={() => setAsking(false)} /> };
}

/** The request for a tidying, in a box of many lines: Enter starts a new one, Ctrl or ⌘ + Enter submits. */
function TidyRequest({ onSubmit, onClose }: { onSubmit(text: string): void; onClose(): void }) {
  const [text, setText] = useState(TIDY);
  const [error, setError] = useState("");
  const area = useRef<HTMLTextAreaElement>(null);
  // At the end of what is there, to add to it.
  useEffect(() => area.current!.setSelectionRange(TIDY.length, TIDY.length), []);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const c = checkJobText(text);
    if (!c.ok) return setError(c.error);
    onSubmit(c.value);
    onClose();
  };
  return (
    <Modal title="AI 整理事项" onClose={onClose} focus={area}>
      <form onSubmit={submit}>
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
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !composing(e)) {
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
            <span className="hint">新建、修改、完成、删除、移动事项，调整先后和顺序都可以，执行前由你逐条确认</span>
          )}
        </label>
        <div className="actions">
          <button type="button" className="text-btn" onClick={onClose}>
            取消
          </button>
          <button className="filled-btn">开始整理</button>
        </div>
      </form>
    </Modal>
  );
}

/** The actions on `folder`, and the panel that moving or tidying it opens; in the root, only making a
 * folder and tidying; none for a folder deleted meanwhile, say on another device. */
function useFolderActions(folder: FolderId | null): { items: MenuItem[]; panel: ReactNode } {
  const { store } = useSession();
  const commit = useCommit();
  const undo = useUndo();
  const { prompt, confirm, snack } = useFeedback();
  const newFolder = useNewFolder();
  const tidy = useTidy(folder);
  const [location, navigate] = useLocation();
  const [moving, setMoving] = useState(false);
  const { board } = useView();
  if (folder === null) {
    const items = [
      { icon: FolderPlus, label: "新建文件夹", onSelect: () => newFolder(null) },
      { icon: Sparkles, label: "AI 整理事项", onSelect: tidy.start },
    ];
    return { items, panel: tidy.panel };
  }
  const f = board.folder(folder);
  if (!f) return { items: [], panel: null };

  const rename = async () => {
    const name = await prompt({ title: "重命名文件夹", label: "名称", value: f.name, ok: "保存", check: checkFolderName });
    if (name !== null && name !== f.name) commit([{ op: "folder.update", id: f.id, name }]);
  };
  const move = (parent: FolderId | null) => {
    if (parent === f.parent_id) return;
    const sort_key = lastKey(store.getView().board.subfolders(parent).map((s) => s.sort_key));
    const r = commit([{ op: "folder.update", id: f.id, parent_id: parent, sort_key }]);
    if (r) snack({ text: `已移至“${board.folderName(parent)}”`, action: "撤销", onAction: () => undo(r.id) });
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

  return {
    items: [
      { icon: FolderPlus, label: "新建子文件夹", onSelect: () => newFolder(f.id) },
      { icon: Sparkles, label: "AI 整理事项", onSelect: tidy.start },
      { icon: Pencil, label: "重命名", onSelect: rename },
      { icon: FolderInput, label: "移动到…", onSelect: () => setMoving(true) },
      { icon: Trash2, label: "删除文件夹", danger: true, onSelect: remove },
    ],
    panel: (
      <>
        {tidy.panel}
        {moving && <FolderPicker title="移动到" current={f.parent_id} exclude={f.id} onPick={move} onClose={() => setMoving(false)} />}
      </>
    ),
  };
}

/** The "…" button of the folder shown, or of the root: what its right-click menu has, and choosing its
 * tasks for [batch operations](docs/ui.md#批量操作). */
export function FolderMenu({ folder }: { folder: FolderId | null }) {
  const root = useContext(RootContext);
  const { items: own, panel } = useFolderActions(folder);
  const { snack } = useFeedback();
  const { board } = useView();
  const batch = useBatch();
  const choose = () => (board.contents(folder).length ? batch.start() : snack("这里没有事项"));
  // After making a folder and tidying, which the root's menu has alone.
  const items = own.length ? [...own.slice(0, 2), { icon: ListChecks, label: "批量操作", onSelect: choose }, ...own.slice(2)] : [];
  const [menu, setMenu] = useState(false);
  // A finger come down on the button, to open the menu once the tap is done.
  const tap = useRef(false);
  useBack("dialog", menu ? () => setMenu(false) : null);
  if (!items.length) return null;
  return (
    <>
      <DropdownMenu.Root open={menu} onOpenChange={setMenu}>
        <Tip label="文件夹操作">
          <DropdownMenu.Trigger
            className="icon-btn"
            // Radix opens the menu as the button goes down. A finger's opens it once the tap is done, as the
            // system's menus do: the press shows, and a scroll or a long press begun on it opens nothing. A
            // click with no pointer behind it, from a screen reader say, opens it too.
            onPointerDown={(e) => {
              tap.current = e.pointerType !== "mouse";
              if (tap.current) e.preventDefault();
            }}
            onClick={(e) => {
              if (tap.current || !(e.nativeEvent as PointerEvent).pointerType) setMenu(true);
              tap.current = false;
            }}
          >
            <Icon of={Ellipsis} />
          </DropdownMenu.Trigger>
        </Tip>
        <DropdownMenu.Portal container={root}>
          <DropdownMenu.Content
            className="menu"
            align="end"
            sideOffset={4}
            // A tap on the button while the menu is open comes down outside it: it closes the menu, and
            // its click opens nothing.
            onPointerDownOutside={() => (tap.current = false)}
          >
            {items.map((item) => (
              <DropdownMenu.Item key={item.label} className={itemClass(item)} onSelect={item.onSelect}>
                <Icon of={item.icon} />
                {item.label}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {panel}
    </>
  );
}

/** Right-clicking `children`, a row standing for the folder (`null` for the root), opens its menu. */
export function FolderContextMenu({ folder, children }: { folder: FolderId | null; children: ReactElement }) {
  const { items, panel } = useFolderActions(folder);
  return (
    <>
      <ContextMenu items={items}>{children}</ContextMenu>
      {panel}
    </>
  );
}
