// What can be done to a folder: make one inside it, rename, move, delete. From its "…" button, or by
// right-clicking it on a computer.

import { DropdownMenu } from "radix-ui";
import { Ellipsis, FolderInput, FolderPlus, Pencil, Trash2 } from "lucide-react";
import { type ReactElement, type ReactNode, useContext, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { lastKey } from "../domain/apply";
import { checkFolderName } from "../domain/text";
import { useCommit, useSession, useUndo, useView } from "../session";
import { useBack } from "../ui/back";
import { RootContext, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { ContextMenu, type MenuItem, itemClass } from "../ui/menu";
import { folderUrl } from "./nav";
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

/** The actions on `folder`, and the picker that moving it opens; in the root, only making a folder;
 * none for a folder deleted meanwhile, say on another device. */
function useFolderActions(folder: FolderId | null): { items: MenuItem[]; picker: ReactNode } {
  const { store } = useSession();
  const commit = useCommit();
  const undo = useUndo();
  const { prompt, confirm, snack } = useFeedback();
  const newFolder = useNewFolder();
  const [location, navigate] = useLocation();
  const [moving, setMoving] = useState(false);
  const { board } = useView();
  if (folder === null) return { items: [{ icon: FolderPlus, label: "新建文件夹", onSelect: () => newFolder(null) }], picker: null };
  const f = board.folder(folder);
  if (!f) return { items: [], picker: null };

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
      { icon: Pencil, label: "重命名", onSelect: rename },
      { icon: FolderInput, label: "移动到…", onSelect: () => setMoving(true) },
      { icon: Trash2, label: "删除文件夹", danger: true, onSelect: remove },
    ],
    picker: moving && <FolderPicker title="移动到" current={f.parent_id} exclude={f.id} onPick={move} onClose={() => setMoving(false)} />,
  };
}

/** The folder's "…" button. */
export function FolderMenu({ folder }: { folder: FolderId }) {
  const root = useContext(RootContext);
  const { items, picker } = useFolderActions(folder);
  const [menu, setMenu] = useState(false);
  useBack("dialog", menu ? () => setMenu(false) : null);
  if (!items.length) return null;
  return (
    <>
      <DropdownMenu.Root open={menu} onOpenChange={setMenu}>
        <DropdownMenu.Trigger className="icon-btn" aria-label="文件夹操作">
          <Icon of={Ellipsis} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal container={root}>
          <DropdownMenu.Content className="menu" align="end" sideOffset={4}>
            {items.map((item) => (
              <DropdownMenu.Item key={item.label} className={itemClass(item)} onSelect={item.onSelect}>
                <Icon of={item.icon} />
                {item.label}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {picker}
    </>
  );
}

/** Right-clicking `children`, a row standing for the folder (`null` for the root), opens its menu. */
export function FolderContextMenu({ folder, children }: { folder: FolderId | null; children: ReactElement }) {
  const { items, picker } = useFolderActions(folder);
  return (
    <>
      <ContextMenu items={items}>{children}</ContextMenu>
      {picker}
    </>
  );
}
