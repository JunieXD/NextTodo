// Panels for choosing: a folder, a task to link, a due date.

import { Check, Folder as FolderIcon, Search } from "lucide-react";
import { useState } from "react";
import type { Due } from "../api/types/Due";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
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
            <button key={f ?? "root"} className={`row${disabled ? " disabled" : ""}`} onClick={() => (onPick(f), onClose())}>
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

/** Picks a task to come before (`pred`) or after (`succ`) `task`. Tasks that would close a loop are
 * shown but cannot be picked. */
export function TaskPicker({ task, dir, onPick, onClose }: { task: Task; dir: "pred" | "succ"; onPick(t: Task): void; onClose(): void }) {
  const { board } = useView();
  const [query, setQuery] = useState("");
  const linked = new Set([task.id, ...(dir === "pred" ? board.before(task) : board.after(task)).map((t) => t.id)]);
  const q = query.trim().toLowerCase();
  const candidates = board
    .sort(board.snapshot.tasks.filter((t) => t.done_at === null && !linked.has(t.id) && t.title.toLowerCase().includes(q)))
    .slice(0, 100);
  return (
    <Modal title={dir === "pred" ? "添加前置" : "添加后续"} onClose={onClose}>
      <label className="search">
        <Icon of={Search} size={18} />
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索事项" autoComplete="off" />
      </label>
      <div className="group picker">
        {candidates.map((t) => {
          const loop = dir === "pred" ? board.chain(task.id, t.id) : board.chain(t.id, task.id);
          return (
            <button key={t.id} className={`row dep${loop ? " disabled" : ""}`} onClick={() => (onPick(t), onClose())}>
              <div className="row-inner">
                <span className={`dot ${board.status(t)}`} />
                <span className="dep-main">
                  <span className="dep-title">{t.title}</span>
                  <span className="dep-path">{loop ? "加上会形成循环" : board.place(t.folder_id)}</span>
                </span>
              </div>
            </button>
          );
        })}
      </div>
      {candidates.length === 0 && <p className="empty">没有可选的事项。</p>}
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
            时间 <span className="hint">可以不填，算当天结束前</span>
          </span>
          <input type="time" value={time} onChange={(e) => setTime(e.target.value)} />
        </label>
        <label className="field">
          提醒
          <select value={time ? remind : ""} disabled={!time} onChange={(e) => setRemind(e.target.value)}>
            <option value="">{time ? "不提醒" : "设了时间才能提醒"}</option>
            {choices.map((m) => (
              <option key={m} value={m}>
                {remindLabel(m)}
              </option>
            ))}
          </select>
        </label>
        <div className="dialog-actions" style={{ padding: "4px 16px 0" }}>
          {due && (
            <button type="button" className="text-btn danger-text" style={{ marginRight: "auto" }} onClick={() => (onSave(null), onClose())}>
              清除截止时间
            </button>
          )}
          <button className="filled-btn">保存</button>
        </div>
      </form>
    </Modal>
  );
}
