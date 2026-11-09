// A tidying's suggestions, each change to be taken or left, and those taken applied as one change
// that can be undone. See docs/ai.md#整理文件夹.

import { useState } from "react";
import type { AiJob } from "../api/types/AiJob";
import type { Board } from "../domain/board";
import { type Group, type Part, groups, ops } from "../domain/tidy";
import { useCommit, useUndo } from "../session";
import { Box } from "../ui/controls";
import { Modal, useFeedback } from "../ui/feedback";

export function Suggestions({ job: j, board, onRetry, onClose }: { job: AiJob; board: Board; onRetry(): void; onClose(): void }) {
  const commit = useCommit();
  const undo = useUndo();
  const { snack } = useFeedback();
  const shown = groups(j, board);
  // What the user picked, by part and by what is there now: taken unless told otherwise, but for what
  // changed since, the user's newer word; changed again, it is asked about again.
  const [picked, setPicked] = useState(() => new Map<string, boolean>());
  const choice = (g: Group, p: Part) => `${g.key} ${p.key}\n${p.current ?? ""}`;
  const isTaken = (g: Group, p: Part) => picked.get(choice(g, p)) ?? p.now === undefined;
  const taken = shown.flatMap((g) => g.parts.filter((p) => isTaken(g, p)));
  const toggle = (g: Group, p: Part) => setPicked((m) => new Map(m).set(choice(g, p), !isTaken(g, p)));

  const apply = () => {
    const r = commit(ops(taken, j, board));
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
      {shown.map((g) => (
        <section key={g.key} className="suggestion">
          <h2 className="group-title">{g.heading}</h2>
          <p className="suggestion-reason">{g.reason}</p>
          <div className="group">
            {g.parts.map((p) => (
              <button key={p.key} className="row suggestion-part" role="checkbox" aria-checked={isTaken(g, p)} onClick={() => toggle(g, p)}>
                <div className="row-inner">
                  <Box checked={isTaken(g, p)} />
                  <span className="suggestion-text">
                    <b>{p.label}</b>
                    {p.text}
                    {p.now !== undefined && <span className="suggestion-now">{p.now}</span>}
                  </span>
                </div>
              </button>
            ))}
          </div>
        </section>
      ))}
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
