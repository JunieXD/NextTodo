// What can be done to a folder: make one inside it, rename, move, delete.

import { DropdownMenu } from "radix-ui";
import { Ellipsis, FolderInput, FolderPlus, Pencil, Trash2 } from "lucide-react";
import { useContext, useState } from "react";
import { useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { lastKey } from "../domain/apply";
import { checkFolderName } from "../domain/text";
import { useCommit, useSession, useUndo, useView } from "../session";
import { useBack } from "../ui/back";
import { RootContext, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { useNewFolder } from "./lists";
import { folderUrl } from "./nav";
import { FolderPicker } from "./pickers";

export function FolderMenu({ folder }: { folder: FolderId }) {
  const root = useContext(RootContext);
  const { store } = useSession();
  const commit = useCommit();
  const undo = useUndo();
  const { prompt, confirm, snack } = useFeedback();
  const newFolder = useNewFolder();
  const [, navigate] = useLocation();
  const [moving, setMoving] = useState(false);
  const [menu, setMenu] = useState(false);
  useBack("dialog", menu ? () => setMenu(false) : null);
  const { board } = useView();
  const f = board.folder(folder);
  if (!f) return null;

  const rename = async () => {
    const name = await prompt({ title: "重命名文件夹", label: "名称", value: f.name, ok: "保存", check: checkFolderName });
    if (name !== null && name !== f.name) commit([{ op: "folder.update", id: f.id, name }]);
  };
  const move = (parent: FolderId | null) => {
    if (parent === f.parent_id) return;
    const sort_key = lastKey(store.getView().board.subfolders(parent).map((s) => s.sort_key));
    const r = commit([{ op: "folder.update", id: f.id, parent_id: parent, sort_key }]);
    if (r) snack({ text: `已移到“${board.folderName(parent)}”`, action: "撤销", onAction: () => undo(r.id) });
  };
  const remove = async () => {
    const inside = [...board.snapshot.tasks].filter((t) => t.folder_id && board.inSubtree(t.folder_id, f.id)).length;
    const body = inside ? `里面的子文件夹和 ${inside} 个事项会一起删除，可以撤销。` : "里面的子文件夹会一起删除，可以撤销。";
    if (!(await confirm({ title: `删除“${f.name}”？`, body, ok: "删除", danger: true }))) return;
    const r = commit([{ op: "folder.delete", id: f.id }]);
    if (!r) return;
    navigate(folderUrl(f.parent_id), { replace: true });
    snack({ text: `已删除“${f.name}”`, action: "撤销", onAction: () => undo(r.id) });
  };

  return (
    <>
      <DropdownMenu.Root open={menu} onOpenChange={setMenu}>
        <DropdownMenu.Trigger className="icon-btn" aria-label="文件夹操作">
          <Icon of={Ellipsis} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal container={root}>
          <DropdownMenu.Content className="menu" align="end" sideOffset={4}>
            <DropdownMenu.Item className="menu-item" onSelect={() => newFolder(f.id)}>
              <Icon of={FolderPlus} />
              新建子文件夹
            </DropdownMenu.Item>
            <DropdownMenu.Item className="menu-item" onSelect={rename}>
              <Icon of={Pencil} />
              重命名
            </DropdownMenu.Item>
            <DropdownMenu.Item className="menu-item" onSelect={() => setMoving(true)}>
              <Icon of={FolderInput} />
              移到…
            </DropdownMenu.Item>
            <DropdownMenu.Item className="menu-item danger-text" onSelect={remove}>
              <Icon of={Trash2} />
              删除文件夹
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {moving && <FolderPicker title="移到文件夹" current={f.parent_id} exclude={f.id} onPick={move} onClose={() => setMoving(false)} />}
    </>
  );
}
