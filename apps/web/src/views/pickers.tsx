// Panels for choosing: a folder, a task to link, a due date.

import { Check, ChevronLeft, ChevronRight, Folder as FolderIcon, Search } from "lucide-react";
import { useRef, useState } from "react";
import type { Due } from "../api/types/Due";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import { matches } from "../domain/board";
import { REMINDERS, isoDate, remindLabel } from "../domain/dates";
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
 * shown but cannot be picked. */
export function TaskPicker({ task, dir, onPick, onClose }: { task: Task; dir: "pred" | "succ"; onPick(t: Task): void; onClose(): void }) {
  const { board } = useView();
  const [query, setQuery] = useState("");
  // `move`: the last step, into a subfolder or up, whose way the folder's page slides in.
  const [nav, setNav] = useState<{ folder: FolderId | null; move?: "forth" | "back" }>({ folder: task.folder_id });
  // Deleted meanwhile, on another device say: from the root.
  const at = board.hasFolder(nav.folder) ? nav.folder : null;
  const folderPage = useRef<HTMLDivElement>(null);
  const go = (folder: FolderId | null, move: "forth" | "back") => {
    setNav({ folder, move });
    // Into another folder, from its top: the top of the modal's body, which scrolls.
    folderPage.current!.parentElement!.scrollTop = 0;
  };
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
    list = found.length ? <div className="group picker">{found.map((t) => row(t, true))}</div> : <p className="empty">未找到包含“{query.trim()}”的事项。</p>;
  } else {
    // Only the folders with something to pick, at any depth.
    const subs = board
      .subfolders(at)
      .map((f) => ({ f, n: choices.filter((t) => board.inSubtree(t.folder_id, f.id)).length }))
      .filter(({ n }) => n > 0);
    const here = board.tasksIn(at).filter(choice);
    list = (
      <div key={at ?? "root"} ref={folderPage} className={nav.move && `page-${nav.move}`}>
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
      <label className="search">
        <Icon of={Search} size={18} />
        {/* Back from the results, the folder's page shows again without sliding in. */}
        <input value={query} onChange={(e) => (setQuery(e.target.value), setNav({ folder: at }))} placeholder="搜索所有文件夹" autoComplete="off" />
      </label>
      {list}
    </Modal>
  );
}

/** Sets or clears the due date, its time and the reminder. */
export function DueEditor({ due, onSave, onClose }: { due: Due | null; onSave(d: Due | null): void; onClose(): void }) {
  const [date, setDate] = useState(due?.date ?? isoDate(new Date()));
  const [time, setTime] = useState(due?.time ?? "");
  const [remind, setRemind] = useState(due?.remind_before?.toString() ?? "");
  // A reminder set elsewhere (say, by an agent) may not be one of the usual choices.
  const choices = [...new Set([...REMINDERS, ...(due?.remind_before === undefined ? [] : [due.remind_before])])].sort((a, b) => a - b);
  const save = (e: React.FormEvent) => {
    e.preventDefault();
    if (!date) return;
    onSave({ date, ...(time && { time }), ...(time && remind && { remind_before: Number(remind) }) });
    onClose();
  };
  return (
    <Modal title="截止时间" onClose={onClose}>
      <form onSubmit={save}>
        <label className="field">
          日期
          <input type="date" value={date} required onChange={(e) => setDate(e.target.value)} />
        </label>
        <label className="field">
          <span>
            时间 <span className="hint">可不填，默认为当天结束</span>
          </span>
          <input type="time" value={time} onChange={(e) => setTime(e.target.value)} />
        </label>
        <label className="field">
          提醒
          <select value={time ? remind : ""} disabled={!time} onChange={(e) => setRemind(e.target.value)}>
            <option value="">{time ? "不提醒" : "设置时间后才能提醒"}</option>
            {choices.map((m) => (
              <option key={m} value={m}>
                {remindLabel(m)}
              </option>
            ))}
          </select>
        </label>
        <div className="actions">
          {due && (
            <button type="button" className="text-btn danger-text" onClick={() => (onSave(null), onClose())}>
              清除截止时间
            </button>
          )}
          <button className="filled-btn">保存</button>
        </div>
      </form>
    </Modal>
  );
}
