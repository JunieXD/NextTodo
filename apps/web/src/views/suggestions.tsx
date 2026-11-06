// A tidying's suggestions, each change to be taken or left, and those taken applied as one change
// that can be undone. See docs/ai.md#整理文件夹.

import { useState } from "react";
import type { AiJob } from "../api/types/AiJob";
import type { Due } from "../api/types/Due";
import type { Op } from "../api/types/Op";
import type { Suggestion } from "../api/types/Suggestion";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { formatDue } from "../domain/dates";
import { useCommit, useUndo } from "../session";
import { Modal, useFeedback } from "../ui/feedback";

/** One change a suggestion makes: to the task's fields, or a task to put before it. */
type Part = { key: string; label: string; text: string } & ({ fields: { title?: string; description?: string; due?: Due } } | { before: TaskId });

/** The changes `s` would still make: to a task not done or gone, and links not there already. */
function parts(s: Suggestion, board: Board): Part[] {
  const t = board.task(s.task);
  if (!t || t.done_at !== null) return [];
  const out: Part[] = [];
  if (s.title !== undefined) out.push({ key: "title", label: "标题", text: s.title, fields: { title: s.title } });
  if (s.description !== undefined) out.push({ key: "description", label: "描述", text: s.description, fields: { description: s.description } });
  if (s.due) out.push({ key: "due", label: "截止", text: formatDue(s.due, new Date()), fields: { due: s.due } });
  for (const id of s.after) {
    const b = board.task(id);
    if (!b || board.before(t).some((x) => x.id === id)) continue;
    out.push({ key: `after ${id}`, label: "前置", text: `#${b.number} ${b.title}`, before: id });
  }
  return out;
}

const key = (task: TaskId, part: string) => `${task} ${part}`;

export function Suggestions({ job: j, board, onRetry, onClose }: { job: AiJob; board: Board; onRetry(): void; onClose(): void }) {
  const commit = useCommit();
  const undo = useUndo();
  const { snack } = useFeedback();
  const shown = (j.suggestions ?? []).map((s) => ({ s, parts: parts(s, board) })).filter((x) => x.parts.length);
  // All taken at first.
  const [left, setLeft] = useState(() => new Set<string>());
  const taken = shown.flatMap(({ s, parts }) => parts.filter((p) => !left.has(key(s.task, p.key))).map((p) => ({ s, p })));
  const toggle = (k: string) =>
    setLeft((l) => {
      const next = new Set(l);
      if (!next.delete(k)) next.add(k);
      return next;
    });

  const apply = () => {
    const ops: Op[] = [];
    for (const { s } of shown) {
      const mine = taken.filter((x) => x.s === s).map((x) => x.p);
      const fields = Object.assign({}, ...mine.map((p) => ("fields" in p ? p.fields : {})));
      if (Object.keys(fields).length) ops.push({ op: "task.update", id: s.task, ...fields });
      for (const p of mine) if ("before" in p) ops.push({ op: "dep.add", before: p.before, after: s.task });
    }
    ops.push({ op: "ai_job.delete", id: j.id });
    const r = commit(ops);
    if (!r) return;
    snack({ text: `已应用 ${taken.length} 处修改`, action: "撤销", onAction: () => undo(r.id) });
    onClose();
  };
  const discard = () => {
    const r = commit([{ op: "ai_job.delete", id: j.id }]);
    if (r) snack({ text: "已放弃整理建议", action: "撤销", onAction: () => undo(r.id) });
    onClose();
  };

  return (
    <Modal title="整理建议" onClose={onClose}>
      <p className="empty">
        {shown.length ? "勾选要采纳的修改，一起应用；应用后可以撤销。" : "暂无可采纳的修改：AI 认为不需要改，或涉及的事项已完成、已删除。"}
      </p>
      {shown.map(({ s, parts }) => {
        const t = board.task(s.task)!;
        return (
          <section key={s.task} className="suggestion">
            <h2 className="group-title">
              #{t.number} {t.title}
            </h2>
            <p className="suggestion-reason">{s.reason}</p>
            <div className="group">
              {parts.map((p) => {
                const k = key(s.task, p.key);
                return (
                  <label key={k} className="row suggestion-part">
                    <div className="row-inner">
                      <input type="checkbox" checked={!left.has(k)} onChange={() => toggle(k)} />
                      <span className="suggestion-text">
                        <b>{p.label}</b>
                        {p.text || "（清空）"}
                      </span>
                    </div>
                  </label>
                );
              })}
            </div>
          </section>
        );
      })}
      <div className="actions">
        <button className="text-btn danger-text" onClick={discard}>
          放弃
        </button>
        <button className="tonal-btn" onClick={() => (onRetry(), onClose())}>
          重新整理
        </button>
        <button className="filled-btn" disabled={!taken.length} onClick={apply}>
          应用 {taken.length} 处
        </button>
      </div>
    </Modal>
  );
}
