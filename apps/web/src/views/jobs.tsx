// AI organizing on screen: the jobs waiting, being organized or failed, and tidyings with suggestions,
// at the top of their folder and of what is next; and what can be done with each. See
// docs/ai.md#整理请求.

import { CircleAlert, FilePlus, ListChecks, LoaderCircle, RotateCcw, Sparkles, Trash2 } from "lucide-react";
import { useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useAiVision } from "../ai";
import type { AiJob } from "../api/types/AiJob";
import type { Op } from "../api/types/Op";
import type { TaskId } from "../api/types/TaskId";
import { isPicture, seesAny, split } from "../attach";
import { lastKey } from "../domain/apply";
import { type Board, cmp } from "../domain/board";
import { blank, checkJobText } from "../domain/text";
import { dueWithDefaults, jobReminders } from "../remind-settings";
import { useCommit, useConnection, useSession, useUndo } from "../session";
import { Modal, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { ContextMenu, type MenuItem } from "../ui/menu";
import { Suggestions } from "./suggestions";

const firstLine = (j: AiJob) => j.text.trim().split("\n")[0];

/** A job's files, in the order they were added. */
const filesOf = (j: AiJob, board: Board) => board.snapshot.attachments.filter((a) => a.job_id === j.id).sort((a, b) => cmp(a.id, b.id));

/** A job's row's title: its text's first line; with none, what its files are (docs/ai.md#附件); for a
 * tidying, what it is. */
function heading(j: AiJob, board: Board): string {
  if (j.kind === "tidy") return "整理已有事项";
  const line = firstLine(j);
  if (line) return line;
  const files = filesOf(j, board);
  if (!files.length) return "没有原文";
  if (files.length === 1) return files[0].name;
  return files.every((a) => isPicture(a.type)) ? `${files.length} 张图片` : `${files.length} 个附件`;
}

/** The title a job's text makes as it is: its first line, or with none its first file's name, without
 * the extension. Empty when there is neither. */
function asIs(j: AiJob, board: Board): string {
  const first = filesOf(j, board)[0];
  const line = firstLine(j) || (first ? split(first.name)[0] : "");
  return [...line.replace(/\t/g, " ").trim()].slice(0, 200).join("");
}

export function originalTaskOps(j: AiJob, board: Board, id: TaskId) {
  const text = j.text.trim();
  const title = asIs(j, board);
  const ops: Op[] = [
    {
      op: "task.create",
      id,
      title,
      ...(text && text !== title && { description: text }),
      sort_key: lastKey(board.contents(j.folder_id).map((t) => t.sort_key)),
      ...(j.folder_id && { folder_id: j.folder_id }),
      ...(j.day && { due: dueWithDefaults(j.day, j.time) }),
    },
    ...filesOf(j, board).map((a): Op => ({ op: "attachment.update", id: a.id, task_id: id })),
    { op: "ai_job.delete", id: j.id },
  ];
  return { title, ops };
}

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
      const reminders = jobReminders();
      commit([{ op: "ai_job.retry", id: j.id, ...(text !== undefined && text !== j.text && { text }), ...(reminders && { reminders }) }]);
    },
    remove(j: AiJob) {
      const r = commit([{ op: "ai_job.delete", id: j.id }]);
      if (r) snack({ text: "已删除整理请求", action: "撤销", onAction: () => undo(r.id) });
    },
    /** Adds the text as it is, its title as `asIs` has it, all of it the description when there is more,
     * with the job's files. */
    addAsIs(j: AiJob, board: Board) {
      const { title, ops } = originalTaskOps(j, board, uuidv7());
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
    ...(j.state === "failed" && j.kind === "organize" && asIs(j, board) ? [{ icon: FilePlus, label: "按原文添加", onSelect: () => actions.addAsIs(j, board) }] : []),
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
            <span className="task-title">{heading(j, board)}</span>
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
  const { user } = useSession();
  const [text, setText] = useState(j.text);
  const failed = j.state === "failed";
  const tidy = j.kind === "tidy";
  const files = filesOf(j, board);
  const pictures = failed && !tidy && files.some((a) => isPicture(a.type));
  const online = useConnection() === "online";
  const vision = useAiVision(user.id, pictures && online);
  // With no text, the task is named after the first file.
  const named = asIs({ ...j, text: "" }, board) !== "";
  /** Does `act` with the text as it stands in the box, which may be empty when `empty` says the job's
   * files are enough: pictures the model sees, or a first file to name the task. */
  const withText = (act: (text: string) => void, empty: boolean) => () => {
    const c = checkJobText(text);
    if (!c.ok) return snack(c.error);
    if (!c.value.trim() && !empty) return snack(tidy ? "要求不能为空" : "内容不能为空");
    act(c.value);
    onClose();
  };
  return (
    <Modal
      title="AI 整理"
      onClose={onClose}
      footer={
        <>
          <button className="text-btn danger-text" onClick={() => (actions.remove(j), onClose())}>
            删除
          </button>
          {failed && (
            <>
              {!tidy && (!blank(text) || named) && (
                <button className="tonal-btn" onClick={withText((text) => actions.addAsIs({ ...j, text }, board), named)}>
                  按原文添加
                </button>
              )}
              <button className="filled-btn" onClick={withText((text) => actions.retry(j, text), pictures && vision !== undefined && seesAny(files, vision))}>
                重试
              </button>
            </>
          )}
        </>
      }
    >
      <p className={`job-detail-status ${j.state}`}>{status(j)}</p>
      <label className="field">
        {tidy ? "要求" : "原文"}
        <textarea className="job-text" value={text} readOnly={!failed} onChange={(e) => setText(e.target.value)} rows={8} />
      </label>
      {!failed && <p className="empty">{j.state === "running" ? "正在整理，完成后可修改。" : `等待整理；整理失败后可修改${tidy ? "要求" : "原文"}并重试。`}</p>}
    </Modal>
  );
}
