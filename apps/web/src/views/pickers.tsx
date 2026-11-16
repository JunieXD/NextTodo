// Panels for choosing: a folder or several, a task to link.

import { Check, ChevronLeft, ChevronRight, Folder as FolderIcon, Plus, Search } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import { matches } from "../domain/board";
import { checkTitle } from "../domain/text";
import { useView } from "../session";
import { Box } from "../ui/controls";
import { Modal } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { FolderName, folderIcon } from "./share";

/** Picks a folder or the root, checking `current`: to add to, the one picked so far; to move to, where
 * what is moved is now, which cannot be picked, nor can where `exclude`, a folder moved, may not go; for
 * a note for AI organizing, any. The folders others share with the user follow, apart. */
export function FolderPicker({
  mode,
  current,
  exclude,
  onPick,
  onClose,
}: {
  mode: "add" | "move" | "note";
  current?: FolderId | null;
  exclude?: FolderId;
  onPick(f: FolderId | null): void;
  onClose(): void;
}) {
  const { board } = useView();
  const row = (f: FolderId | null) => {
    const disabled = (mode === "move" && f === current) || (exclude !== undefined && !board.canMove(exclude, f));
    return (
      <button key={f ?? "root"} className="row" disabled={disabled} onClick={() => (onPick(f), onClose())}>
        <div className="row-inner" style={{ paddingLeft: 16 + Math.max(0, board.depth(f) - 1) * 22 }}>
          <span className="folder-ic">
            <Icon of={folderIcon(board, f)} size={18} />
          </span>
          <span className="folder-main">
            <span>
              <FolderName board={board} id={f} />
            </span>
          </span>
          {current === f && (
            <span className="text-primary">
              <Icon of={Check} />
            </span>
          )}
        </div>
      </button>
    );
  };
  const tree = board.folderTree();
  const theirs = tree.filter((f) => board.ownerOf(f) !== board.me);
  return (
    <Modal title={{ add: "添加到", move: "移动到", note: "选择文件夹" }[mode]} onClose={onClose}>
      <div className="group picker">{tree.filter((f) => board.ownerOf(f) === board.me).map(row)}</div>
      {theirs.length > 0 && (
        <>
          <h2 className="group-title">共享给我</h2>
          <div className="group picker">{theirs.map(row)}</div>
        </>
      )}
    </Modal>
  );
}

/** Ticks folders, each with the folders inside it; the root is all of them. A folder inside one ticked
 * is ticked with it, and can't be on its own. The folders others share with the user follow, apart. */
export function FolderChecks({
  title,
  chosen,
  onChange,
  onClose,
}: {
  title: string;
  chosen: (FolderId | null)[];
  onChange(to: (FolderId | null)[]): void;
  onClose(): void;
}) {
  const { board } = useView();
  const row = (f: FolderId | null) => {
    const on = chosen.includes(f);
    const within = chosen.some((c) => c !== f && board.inSubtree(f, c));
    // Ticked, a folder takes the place of those inside it.
    const toggle = () => onChange(on ? chosen.filter((c) => c !== f) : [...chosen.filter((c) => !board.inSubtree(c, f)), f]);
    return (
      <button key={f ?? "root"} className="row" role="checkbox" aria-checked={on || within} disabled={within} onClick={toggle}>
        <div className="row-inner" style={{ paddingLeft: 16 + Math.max(0, board.depth(f) - 1) * 22 }}>
          <span className="folder-ic">
            <Icon of={folderIcon(board, f)} size={18} />
          </span>
          <span className="folder-main">
            <span>
              <FolderName board={board} id={f} />
            </span>
          </span>
          <Box checked={on || within} />
        </div>
      </button>
    );
  };
  const tree = board.folderTree();
  const theirs = tree.filter((f) => board.ownerOf(f) !== board.me);
  return (
    <Modal title={title} onClose={onClose}>
      <div className="group picker">{tree.filter((f) => board.ownerOf(f) === board.me).map(row)}</div>
      {theirs.length > 0 && (
        <>
          <h2 className="group-title">共享给我</h2>
          <div className="group picker">{theirs.map(row)}</div>
        </>
      )}
    </Modal>
  );
}

/** Picks a task to come before (`pred`) or after (`succ`) `task`, of those it can be linked to, in the
 * same place (docs/design.md#依赖): folder by folder, as the folders page has them, starting in the
 * task's own and up to the shared folder it is in, if any; or by searching them all. Tasks that would
 * close a loop are shown but cannot be picked. One not there yet is created from the search, in the
 * folder the panel is at. */
export function TaskPicker({
  task,
  dir,
  onPick,
  onCreate,
  onClose,
}: {
  task: Task;
  dir: "pred" | "succ";
  onPick(t: Task): void;
  onCreate(title: string, folder: FolderId | null): void;
  onClose(): void;
}) {
  const { board } = useView();
  const [query, setQuery] = useState("");
  // `move`: the last step, into a subfolder or up, whose way the folder's page slides in.
  const [nav, setNav] = useState<{ folder: FolderId | null; move?: "forth" | "back" }>({ folder: task.folder_id });
  const top = board.sharedRoot(task.folder_id) ?? null;
  // Deleted meanwhile, on another device say: from the top.
  const at = board.hasFolder(nav.folder) ? nav.folder : top;
  const go = (folder: FolderId | null, move: "forth" | "back") => setNav({ folder, move });
  const field = useRef<HTMLLabelElement>(null);
  const searching = query.trim() !== "";
  // Another folder, or back at the root as this one went, and the results or the folder again, from
  // the top: of the modal's body, which scrolls, and nothing around it, the panel still sliding in.
  useLayoutEffect(() => {
    field.current!.parentElement!.scrollTo(0, 0);
  }, [at, searching]);
  const linked = new Set([task.id, ...(dir === "pred" ? board.before(task) : board.after(task)).map((t) => t.id)]);
  const choice = (t: Task) => t.done_at === null && !linked.has(t.id) && board.together(t, task);
  const choices = board.snapshot.tasks.filter(choice);
  const row = (t: Task, path: boolean) => {
    const loop = dir === "pred" ? board.chain(task.id, t.id) : board.chain(t.id, task.id);
    const note = loop ? "添加后将形成循环" : path && board.place(t.folder_id);
    return (
      <button key={t.id} className="row dep" disabled={!!loop} onClick={() => (onPick(t), onClose())}>
        <div className="row-inner">
          <span className={`dot ${board.status(t)}`} />
          <span className="dep-main">
            <span className="dep-title">{t.title}</span>
            {note && <span className="dep-path">{note}</span>}
          </span>
        </div>
      </button>
    );
  };

  const q = query.trim().toLowerCase();
  let list;
  if (q) {
    const found = board.sort(choices.filter((t) => matches(t, q))).slice(0, 100);
    const title = checkTitle(query);
    list = (
      <>
        {(title.ok || found.length > 0) && (
          <div className="group picker">
            {title.ok && (
              <button className="row dep add-row" onClick={() => (onCreate(title.value, at), onClose())}>
                <div className="row-inner">
                  <Icon of={Plus} />
                  <span className="dep-main">
                    <span className="dep-title">新建“{title.value}”</span>
                    <span className="dep-path">放在{board.place(at)}</span>
                  </span>
                </div>
              </button>
            )}
            {found.map((t) => row(t, true))}
          </div>
        )}
        {!found.length && <p className="empty">未找到包含“{query.trim()}”的事项。</p>}
      </>
    );
  } else {
    // Only the folders with something to pick, at any depth.
    const subs = board
      .subfolders(at)
      .map((f) => ({ f, n: choices.filter((t) => board.inSubtree(t.folder_id, f.id)).length }))
      .filter(({ n }) => n > 0);
    const here = board.tasksIn(at).filter(choice);
    list = (
      // Gone back to the root by itself, it doesn't slide in: that answers no step of the user's.
      <div key={at ?? "root"} className={at === nav.folder && nav.move ? `page-${nav.move}` : undefined}>
        <h2 className="group-title">{board.place(at)}</h2>
        <div className="group picker">
          {at !== null && at !== top && (
            <button className="row" onClick={() => go(board.parent(at), "back")}>
              <div className="row-inner">
                <span className="chev">
                  <Icon of={ChevronLeft} size={18} />
                </span>
                <span className="folder-main">上一级</span>
              </div>
            </button>
          )}
          {subs.map(({ f, n }) => (
            <button key={f.id} className="row" onClick={() => go(f.id, "forth")}>
              <div className="row-inner">
                <span className="folder-ic">
                  <Icon of={FolderIcon} size={18} />
                </span>
                <span className="folder-main">
                  <span>{f.name}</span>
                  <span className="folder-sub">{n} 项可选</span>
                </span>
                <span className="chev">
                  <Icon of={ChevronRight} size={18} />
                </span>
              </div>
            </button>
          ))}
          {here.map((t) => row(t, false))}
        </div>
        {subs.length + here.length === 0 && <p className="empty">{at === top ? "暂无可选的事项。" : "这里没有可选的事项。"}</p>}
      </div>
    );
  }
  return (
    <Modal title={dir === "pred" ? "添加前置" : "添加后续"} onClose={onClose}>
      <label className="search" ref={field}>
        <Icon of={Search} size={18} />
        {/* Back from the results, the folder's page shows again without sliding in. */}
        <input value={query} onChange={(e) => (setQuery(e.target.value), setNav({ folder: at }))} placeholder="搜索所有文件夹" autoComplete="off" />
      </label>
      {list}
    </Modal>
  );
}
