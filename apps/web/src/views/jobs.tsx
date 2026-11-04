// AI organizing on screen: the jobs waiting, being organized or failed, at the top of their folder and
// of what is next; and what can be done with each. See docs/ai.md#整理请求.

import { CircleAlert, FilePlus, LoaderCircle, RotateCcw, Sparkles, Trash2 } from "lucide-react";
import { useState } from "react";
import { v7 as uuidv7 } from "uuid";
import type { AiJob } from "../api/types/AiJob";
import type { Op } from "../api/types/Op";
import { lastKey } from "../domain/apply";
import type { Board } from "../domain/board";
import { checkJobText } from "../domain/text";
import { useCommit, useUndo } from "../session";
import { Modal, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { ContextMenu, type MenuItem } from "../ui/menu";

const firstLine = (j: AiJob) => j.text.trim().split("\n")[0];

function status(j: AiJob): string {
  if (j.state === "waiting") return "等待整理";
  if (j.state === "running") return "正在整理";
  return `整理失败：${j.error}`;
}

const STATE_ICONS = { waiting: Sparkles, running: LoaderCircle, failed: CircleAlert };

/** What can be done with a job, here and in its details. */
function useJobActions() {
  const commit = useCommit();
  const undo = useUndo();
  const { snack } = useFeedback();
  return {
    retry(j: AiJob, text?: string) {
      commit([{ op: "ai_job.retry", id: j.id, ...(text !== undefined && text !== j.text && { text }) }]);
    },
    remove(j: AiJob) {
      const r = commit([{ op: "ai_job.delete", id: j.id }]);
      if (r) snack({ text: "已删除整理请求", action: "撤销", onAction: () => undo(r.id) });
    },
    /** Adds the text as it is: its first line the title, all of it the description when there is more. */
    addAsIs(j: AiJob, board: Board) {
      const text = j.text.trim();
      const title = [...firstLine(j).replace(/\t/g, " ").trim()].slice(0, 200).join("");
      const ops: Op[] = [
        {
          op: "task.create",
          id: uuidv7(),
          title,
          ...(text !== title && { description: text }),
          sort_key: lastKey(board.contents(j.folder_id).map((t) => t.sort_key)),
          ...(j.folder_id && { folder_id: j.folder_id }),
        },
        { op: "ai_job.delete", id: j.id },
      ];
      const r = commit(ops);
      if (r) snack({ text: `已添加“${title}”`, action: "撤销", onAction: () => undo(r.id) });
    },
  };
}

/** Rows for `jobs`, inside a group; `path` shows each one's folder. */
export function JobRows({ jobs, board, path }: { jobs: AiJob[]; board: Board; path?: boolean }) {
  const [open, setOpen] = useState<AiJob | null>(null);
  // The job as it is now, or gone.
  const shown = open && board.snapshot.ai_jobs.find((j) => j.id === open.id);
  return (
    <>
      {jobs.map((j) => (
        <JobRow key={j.id} job={j} board={board} path={path} onOpen={() => setOpen(j)} />
      ))}
      {shown && <JobDetails job={shown} board={board} onClose={() => setOpen(null)} />}
    </>
  );
}

function JobRow({ job: j, board, path, onOpen }: { job: AiJob; board: Board; path?: boolean; onOpen(): void }) {
  const actions = useJobActions();
  const menu: MenuItem[] = [
    ...(j.state === "failed"
      ? [
          { icon: RotateCcw, label: "重试", onSelect: () => actions.retry(j) },
          { icon: FilePlus, label: "按原文添加", onSelect: () => actions.addAsIs(j, board) },
        ]
      : []),
    { icon: Trash2, label: "删除", danger: true, onSelect: () => actions.remove(j) },
  ];
  return (
    <ContextMenu items={menu}>
      <div className={`row job ${j.state}`} onClick={onOpen}>
        <div className="row-inner">
          <span className="job-ic">
            <Icon of={STATE_ICONS[j.state]} size={20} />
          </span>
          <button type="button" className="task-main">
            <span className="task-title">{firstLine(j)}</span>
            <span className="task-meta">
              {path && j.folder_id && <span className="m-path">{board.place(j.folder_id)}</span>}
              <span className="job-status">{status(j)}</span>
            </span>
          </button>
          {j.state === "failed" && (
            <button
              type="button"
              className="text-btn"
              onClick={(e) => {
                e.stopPropagation();
                actions.retry(j);
              }}
            >
              重试
            </button>
          )}
        </div>
      </div>
    </ContextMenu>
  );
}

/** The whole text and how the job stands; a failed one's text can be changed before retrying. */
function JobDetails({ job: j, board, onClose }: { job: AiJob; board: Board; onClose(): void }) {
  const actions = useJobActions();
  const { snack } = useFeedback();
  const [text, setText] = useState(j.text);
  const failed = j.state === "failed";
  /** Does `act` with the text as it stands in the box. */
  const withText = (act: (text: string) => void) => () => {
    const c = checkJobText(text);
    if (!c.ok) return snack(c.error);
    act(c.value);
    onClose();
  };
  return (
    <Modal title="AI 整理" onClose={onClose}>
      <p className={`job-detail-status ${j.state}`}>{status(j)}</p>
      <label className="field">
        原文
        <textarea className="job-text" value={text} readOnly={!failed} onChange={(e) => setText(e.target.value)} rows={8} />
      </label>
      {!failed && <p className="empty">{j.state === "running" ? "正在整理，整理完再改。" : "等待整理；整理失败后可以修改原文再试。"}</p>}
      <div className="actions">
        <button className="text-btn danger-text" onClick={() => (actions.remove(j), onClose())}>
          删除
        </button>
        {failed && (
          <>
            <button className="tonal-btn" onClick={withText((text) => actions.addAsIs({ ...j, text }, board))}>
              按原文添加
            </button>
            <button className="filled-btn" onClick={withText((text) => actions.retry(j, text))}>
              重试
            </button>
          </>
        )}
      </div>
    </Modal>
  );
}
