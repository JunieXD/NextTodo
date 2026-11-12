// Panels for choosing: a folder, a task to link.

import { Check, ChevronLeft, ChevronRight, Folder as FolderIcon, Plus, Search } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import { matches } from "../domain/board";
import { checkTitle } from "../domain/text";
import { useView } from "../session";
import { Modal } from "../ui/feedback";
import { Icon } from "../ui/icons";

/** Picks a folder or the root. Folders inside `exclude` (and `exclude` itself) cannot be picked. */
export function FolderPicker({
  title,
  current,
  exclude,
  onPick,
  onClose,
}: {
  title: string;
  current: FolderId | null;
  exclude?: FolderId;
  onPick(f: FolderId | null): void;
  onClose(): void;
}) {
  const { board } = useView();
  return (
    <Modal title={title} onClose={onClose}>
      <div className="group picker">
        {board.folderTree().map((f) => {
          const disabled = exclude !== undefined && f !== null && board.inSubtree(f, exclude);
          return (
            <button key={f ?? "root"} className="row" disabled={disabled} onClick={() => (onPick(f), onClose())}>
              <div className="row-inner" style={{ paddingLeft: 16 + Math.max(0, board.depth(f) - 1) * 22 }}>
                <span className="folder-ic">
                  <Icon of={FolderIcon} size={18} />
                </span>
                <span className="folder-main">{board.folderName(f)}</span>
                {current === f && (
                  <span className="text-primary">
                    <Icon of={Check} />
                  </span>
                )}
              </div>
            </button>
          );
        })}
      </div>
    </Modal>
  );
}

/** Picks a task to come before (`pred`) or after (`succ`) `task`: folder by folder, as the folders page
 * has them, starting in the task's own, or by searching them all. Tasks that would close a loop are
 * shown but cannot be picked. One not there yet is created from the search, in the folder the panel
 * is at. */
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
  // Deleted meanwhile, on another device say: from the root.
  const at = board.hasFolder(nav.folder) ? nav.folder : null;
  const go = (folder: FolderId | null, move: "forth" | "back") => setNav({ folder, move });
  const field = useRef<HTMLLabelElement>(null);
  const searching = query.trim() !== "";
  // Another folder, or back at the root as this one went, and the results or the folder again, from
  // the top: of the modal's body, which scrolls, and nothing around it, the panel still sliding in.
  useLayoutEffect(() => {
    field.current!.parentElement!.scrollTo(0, 0);
  }, [at, searching]);
  const linked = new Set([task.id, ...(dir === "pred" ? board.before(task) : board.after(task)).map((t) => t.id)]);
  const choice = (t: Task) => t.done_at === null && !linked.has(t.id);
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
          {at !== null && (
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
        {subs.length + here.length === 0 && <p className="empty">{at === null ? "暂无可选的事项。" : "这里没有可选的事项。"}</p>}
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
