// AI organizing on screen: the jobs waiting, being organized or failed, and tidyings with suggestions,
// at the top of their folder and of what is next; and what can be done with each. See
// docs/ai.md#整理请求.

import { CircleAlert, FilePlus, ListChecks, LoaderCircle, RotateCcw, Sparkles, Trash2 } from "lucide-react";
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
import { Suggestions } from "./suggestions";

const firstLine = (j: AiJob) => j.text.trim().split("\n")[0];

/** A job's row's title: its text's first line, or for a tidying, what it is. */
const heading = (j: AiJob) => (j.kind === "tidy" ? "整理已有事项" : firstLine(j));

function status(j: AiJob): string {
  if (j.state === "waiting") return "等待整理";
  if (j.state === "running") return "正在整理";
  if (j.state === "ready") return j.suggestions!.length ? `${j.suggestions!.length} 条建议，待确认` : "无需修改";
  return `整理失败：${j.error}`;
}

const STATE_ICONS = { waiting: Sparkles, running: LoaderCircle, failed: CircleAlert, ready: ListChecks };

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
  const actions = useJobActions();
  const [open, setOpen] = useState<AiJob | null>(null);
  // The job as it is now, or gone.
  const shown = open && board.snapshot.ai_jobs.find((j) => j.id === open.id);
  const close = () => setOpen(null);
  return (
    <>
      {jobs.map((j) => (
        <JobRow key={j.id} job={j} board={board} path={path} onOpen={() => setOpen(j)} />
      ))}
      {shown &&
        (shown.state === "ready" ? (
          <Suggestions job={shown} board={board} onRetry={() => actions.retry(shown)} onClose={close} />
        ) : (
          <JobDetails job={shown} board={board} onClose={close} />
        ))}
    </>
  );
}

function JobRow({ job: j, board, path, onOpen }: { job: AiJob; board: Board; path?: boolean; onOpen(): void }) {
  const actions = useJobActions();
  const menu: MenuItem[] = [
    ...(j.state === "failed" ? [{ icon: RotateCcw, label: "重试", onSelect: () => actions.retry(j) }] : []),
    ...(j.state === "failed" && j.kind === "organize" ? [{ icon: FilePlus, label: "按原文添加", onSelect: () => actions.addAsIs(j, board) }] : []),
    ...(j.state === "ready"
      ? [
          { icon: ListChecks, label: "查看建议", onSelect: onOpen },
          { icon: RotateCcw, label: "重新整理", onSelect: () => actions.retry(j) },
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
            <span className="task-title">{heading(j)}</span>
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
          {j.state === "ready" && (
            <button type="button" className="text-btn">
              查看
            </button>
          )}
        </div>
      </div>
    </ContextMenu>
  );
}

/** The whole text, or what a tidying asks for, and how the job stands; a failed one's text can be
 * changed before retrying. */
function JobDetails({ job: j, board, onClose }: { job: AiJob; board: Board; onClose(): void }) {
  const actions = useJobActions();
  const { snack } = useFeedback();
  const [text, setText] = useState(j.text);
  const failed = j.state === "failed";
  const tidy = j.kind === "tidy";
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
        {tidy ? "要求" : "原文"}
        <textarea className="job-text" value={text} readOnly={!failed} onChange={(e) => setText(e.target.value)} rows={8} />
      </label>
      {!failed && <p className="empty">{j.state === "running" ? "正在整理，整理完再改。" : `等待整理；整理失败后可以修改${tidy ? "要求" : "原文"}再试。`}</p>}
      <div className="actions">
        <button className="text-btn danger-text" onClick={() => (actions.remove(j), onClose())}>
          删除
        </button>
        {failed && (
          <>
            {!tidy && (
              <button className="tonal-btn" onClick={withText((text) => actions.addAsIs({ ...j, text }, board))}>
                按原文添加
              </button>
            )}
            <button className="filled-btn" onClick={withText((text) => actions.retry(j, text))}>
              重试
            </button>
          </>
        )}
      </div>
    </Modal>
  );
}
