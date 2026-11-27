// A task's details: title, folder, who is to do it, due date, description, attachments, what it
// waits for and what waits for it.

import { ArrowUpRight, Calendar, CalendarCheck, CalendarClock, Folder as FolderIcon, KeyRound, type LucideIcon, Plus, Repeat, User, Users, X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useLocation } from "wouter";
import type { Attachment } from "../api/types/Attachment";
import type { Completion } from "../api/types/Completion";
import type { MutationId } from "../api/types/MutationId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { TaskFields } from "../domain/apply";
import type { Board } from "../domain/board";
import { lateness } from "../domain/calendar";
import { day, dueAfterLabel, dueIn, formatDay, formatDue, formatMoment, isoDate, startBeforeLabel, startLabel } from "../domain/dates";
import { daysBefore, doneToday, repeatText, skippable } from "../domain/repeat";
import { draggingFiles, filesIn, reference } from "../attach";
import { checkDescription } from "../domain/text";
import { useCommit, useSession, useUndo, useView } from "../session";
import { Check } from "../ui/controls";
import { Modal, Tip, useFeedback } from "../ui/feedback";
import { useWide } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { useTaskActions } from "./actions";
import { useAddFiles, useAttachments } from "./attachments";
import { DueEditor, StartEditor } from "./due";
import { FolderGlyph, styled } from "./folder-look";
import { useMarks } from "./marks";
import { folderUrl, useSelection } from "./nav";
import { FolderPicker, TaskPicker } from "./pickers";
import { ReminderRow } from "./remind";
import { dueClass } from "./rows";
import { AssigneePicker, Avatars, namesOf } from "./share";
import { startTipSeen } from "./start-tip";
import { source, Title, DescriptionSection } from "./detail-text";

const STATUS = { ready: "可做", waiting: "等待中", done: "已完成", dropped: "已放弃" };

type Panel = "folder" | "assignees" | "start" | "due" | "pred" | "succ" | "completions" | null;

/** Who made a change, followed by a space before the Chinese. */
const who = (board: Board, user: string, via: string | null) => {
  const s = source(board, user, via);
  return s ? `${s} ` : "";
};

/** `closing`: the desktop pane is sliding away with this task still in it. Its panels close at once,
 * so their focus trap does not hold on to focus that has moved on, such as to a quick add. */
export function Detail({ id, now, closing, onClose }: { id: TaskId; now: Date; closing?: boolean; onClose?: () => void }) {
  const { board } = useView();
  const t = board.task(id);
  const actions = useTaskActions();
  const commit = useCommit();
  const { snack } = useFeedback();
  const { pop } = useMarks();
  const { select } = useSelection();
  const [location] = useLocation();
  const wide = useWide();
  const [panel, setPanel] = useState<Panel>(null);
  if (closing && panel) setPanel(null);
  const update = (fields: TaskFields) => commit([{ op: "task.update", id, ...fields }]);
  const { store } = useSession();
  const undo = useUndo();
  /** Puts text where the description is being edited, while it is. */
  const editing = useRef<((text: string) => void) | null>(null);
  const refer = (a: Attachment) => {
    const ref = reference(a);
    if (editing.current) editing.current(ref);
    else {
      // On a line of its own after what is there.
      const description = store.getView().board.task(id)!.description.trimEnd();
      const c = checkDescription(description ? `${description}\n\n${ref}` : ref);
      if (!c.ok) return snack(c.error);
      if (!update({ description: c.value })) return;
    }
    snack("已在描述中引用");
  };
  const addFiles = useAddFiles();
  const add = (files: File[]) => addFiles(id, files);
  const attachments = useAttachments(id, refer, add);
  const dropping = useFileDrop(id, add);
  if (!t) return <div className="detail-empty">该事项已删除</div>;
  const status = board.status(t);
  const shared = board.sharedRoot(t.folder_id);
  const countdown = t.due && t.done_at === null ? dueIn(t.due, now) : null;
  // The deadline of a task after it that it is to make (docs/design/dates.md#后续的截止).
  const by = board.dueBy().get(t.id);
  const found = t.completions && store.completedAt(t.id, t.completions.count);
  // While it repeats, or done for good by that last time itself; not once the repeat ended otherwise,
  // it was given up, or it was reopened and done again, which taking back an earlier time would undo
  // too.
  const takesBack = t.done_at === null ? t.due?.repeat !== undefined : !t.dropped && found !== undefined && found === store.doneBy(t.id);
  const lastDone = takesBack ? found : undefined;
  const undoLastDone = (m: MutationId) => {
    const r = commit([{ op: "undo", mutation: m }]);
    if (r) snack({ text: "已撤销上一次完成", action: "撤销", onAction: () => undo(r.id) });
  };

  return (
    <div className="detail" {...dropping.handlers}>
      {dropping.on && <div className="drop-cover">松开添加附件</div>}
      <div className="d-head" data-drag>
        <Check status={status} large pop={pop === t.id} onToggle={() => actions.toggle(t)} />
        <div className="d-title-wrap">
          <Title key={t.id} task={t} onSave={(title) => update({ title })} />
          <div className="d-status">
            <span className={`pill ${status}`}>{STATUS[status]}</span>
            {/* What it waits for. */}
            {status === "waiting" && board.waitingOn(t) > 0 && <span>还有 {board.waitingOn(t)} 项前置未完成</span>}
            {status === "waiting" && board.notStarted(t) && <span>{startLabel(t.start!, t.start_time, now)}</span>}
            {t.number > 0 && <span className="tabular-nums">{board.label(t)}</span>}
          </div>
        </div>
        {onClose && (
          <Tip label="关闭">
            <button className="icon-btn" onClick={onClose}>
              <Icon of={X} />
            </button>
          </Tip>
        )}
      </div>

      <div className="group">
        <Prop
          icon={FolderIcon}
          label="文件夹"
          value={
            <>
              {t.folder_id && styled(board, t.folder_id) && <FolderGlyph board={board} id={t.folder_id} size={14} />}
              {board.place(t.folder_id)}
            </>
          }
          onClick={() => setPanel("folder")}
          side={
            // On a phone, not while the folder is the page under the details.
            (wide || location !== folderUrl(t.folder_id)) && (
              <Tip label="在文件夹中显示">
                <button className="prop-side" onClick={() => actions.reveal(t)}>
                  <Icon of={ArrowUpRight} size={18} />
                </button>
              </Tip>
            )
          }
        />
        {shared && (
          <Prop
            icon={Users}
            label="负责人"
            value={
              t.assignees.length > 1 ? (
                <>
                  <Avatars board={board} ids={t.assignees} size={22} max={3} />
                  {namesOf(board, t.assignees)}
                </>
              ) : t.assignees.length ? (
                board.name(t.assignees[0])
              ) : (
                "未指定（所有成员）"
              )
            }
            cls={t.assignees.length ? "" : "muted"}
            onClick={() => setPanel("assignees")}
          />
        )}
        <Prop
          icon={CalendarClock}
          label="开始"
          value={
            !t.start
              ? "未设置"
              : t.due?.repeat
                ? startBeforeLabel(daysBefore(t.start, t.due.date), t.start_time)
                : formatDue({ date: t.start, time: t.start_time }, now)
          }
          cls={t.start ? "" : "muted"}
          onClick={() => setPanel("start")}
        />
        <Prop
          icon={Calendar}
          label="截止"
          value={
            t.due ? (
              <>
                {formatDue(t.due, now)}
                {t.due_after && <span className="muted">（{dueAfterLabel(t.due_after)}）</span>}
                {countdown && <span className={dueClass(t, now) === "overdue" ? "" : "muted"}> · {countdown}</span>}
              </>
            ) : t.due_after ? (
              dueAfterLabel(t.due_after)
            ) : (
              "未设置"
            )
          }
          note={
            by &&
            (t.due ? (
              <span className="m-warn">
                晚于后续“{by.title}”的截止（{formatDue(by.due, now)}）
              </span>
            ) : (
              `后续“${by.title}”${formatDue(by.due, now)}${by.due.time ? " " : ""}截止`
            ))
          }
          cls={t.due ? dueClass(t, now) : t.due_after ? "" : "muted"}
          onClick={() => setPanel("due")}
        />
        {t.done_at === null && <ReminderRow task={t} now={now} update={update} />}
        {t.due?.repeat && <Prop icon={Repeat} label="重复" value={repeatText(t.due.repeat, t.due.date, now)} onClick={() => setPanel("due")} />}
        {(t.due?.repeat || t.completions) && (
          <Prop
            icon={CalendarCheck}
            label="已完成"
            value={completionsLabel(t, now)}
            cls={t.completions ? "" : "muted"}
            onClick={t.completions ? () => setPanel("completions") : undefined}
          />
        )}
      </div>

      <DescriptionSection
        key={t.id}
        task={t}
        attachments={board.attachments(t.id)}
        onEditing={(insert) => (editing.current = insert)}
        onSave={(description) => update({ description })}
        onFiles={add}
        onAttachment={attachments.open}
      />
      {attachments.section}

      <div className="d-sec">
        <div>
          <h3>前置</h3>
          <p>以下事项完成后，才能开始此项</p>
        </div>
      </div>
      <Links board={board} list={board.before(t)} onOpen={(o) => select(o.id)} onRemove={(o) => actions.unlink(o.id, t.id)} onAdd={() => setPanel("pred")} add="添加前置" />
      <div className="d-sec">
        <div>
          <h3>后续</h3>
          <p>此项完成后，才能开始以下事项</p>
        </div>
      </div>
      <Links board={board} list={board.after(t)} onOpen={(o) => select(o.id)} onRemove={(o) => actions.unlink(t.id, o.id)} onAdd={() => setPanel("succ")} add="添加后续" />

      <p className="provenance">
        <Icon of={t.created_via ? KeyRound : User} size={14} />
        {who(board, t.created_by, t.created_via)}创建于 {formatMoment(t.created_at, now)}
        {t.updated_at !== t.created_at && `，${who(board, t.updated_by, t.updated_via)}修改于 ${formatMoment(t.updated_at, now)}`}
      </p>
      <div className="group detail-actions">
        {lastDone && (
          <button className="row end-repeat" onClick={() => undoLastDone(lastDone)}>
            <div className="row-inner">撤销上一次完成</div>
          </button>
        )}
        {skippable(t, isoDate(now)) && (
          <button className="row end-repeat" onClick={() => actions.skip(t)}>
            <div className="row-inner">跳过这一次</div>
          </button>
        )}
        {t.due?.repeat && (
          <button className="row end-repeat" onClick={() => actions.endRepeat(t)}>
            <div className="row-inner">结束重复并完成</div>
          </button>
        )}
        {t.done_at === null && (
          <button className="row end-repeat" onClick={() => actions.drop(t)}>
            <div className="row-inner">放弃事项</div>
          </button>
        )}
        <button className="row danger" onClick={() => actions.remove(t)}>
          <div className="row-inner">删除事项</div>
        </button>
      </div>

      {panel === "folder" && (
        <FolderPicker
          mode="move"
          current={t.folder_id}
          onPick={(f) => actions.move(t, f)}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "assignees" && shared && (
        <AssigneePicker
          folder={shared}
          chosen={t.assignees}
          onToggle={(user, on) => update(on ? { assign: [user] } : { unassign: [user] })}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "start" && (
        <StartEditor
          start={t.start}
          time={t.start_time}
          due={t.due}
          dueAfter={t.due_after}
          onSave={(s) => {
            if (!update(s ? { start: s.date, start_time: s.time ?? null } : { start: null }) || !s) return;
            startTipSeen(board.me);
          }}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "due" && (
        <DueEditor
          due={t.due}
          dueAfter={t.due_after}
          start={t.start}
          startTime={t.start_time}
          endedAs={status === "done" || status === "dropped" ? STATUS[status] : null}
          others={(t.start_reminders?.length ?? 0) + (t.remind_at?.length ?? 0)}
          onSave={update}
          onClose={() => setPanel(null)}
        />
      )}
      {(panel === "pred" || panel === "succ") && (
        <TaskPicker
          task={t}
          dir={panel}
          onPick={(o) => (panel === "pred" ? actions.link(o.id, t.id) : actions.link(t.id, o.id))}
          onCreate={(title, folder) => actions.linkNew(t, panel, title, folder)}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "completions" && t.completions && (
        <Completions records={board.completions(t.id)} count={t.completions.count} now={now} onClose={() => setPanel(null)} />
      )}
    </div>
  );
}

/** The times a repeating task was done, the latest first, `count` in all: those done before there were
 * records are only counted. */
function Completions({ records, count, now, onClose }: { records: Completion[]; count: number; now: Date; onClose(): void }) {
  const unrecorded = count - records.length;
  return (
    <Modal title="完成记录" onClose={onClose}>
      <div className="group">
        {records.map((c) => (
          <div key={`${c.due} ${c.again}`} className="row prop">
            <div className="row-inner controls">
              <span className="row-name">{formatDay(day(c.done).getTime(), now)}</span>
              <span className="row-value">
                截止{formatDue({ date: c.due }, now)} · {lateness(c)}
              </span>
            </div>
          </div>
        ))}
      </div>
      {unrecorded > 0 && <p className="group-note">更早的 {unrecorded} 次没有记录</p>}
    </Modal>
  );
}

/** A property of the task; one without `onClick` is only shown. `side` is a button of its own at the
 * end, beside the part that takes the click. */
function Prop({
  icon,
  label,
  value,
  note,
  cls = "",
  onClick,
  side,
}: {
  icon: LucideIcon;
  label: string;
  value: ReactNode;
  /** A line under the value, smaller. */
  note?: ReactNode;
  cls?: string;
  onClick?(): void;
  side?: ReactNode;
}) {
  const parts = (
    <>
      <Icon of={icon} />
      <span className="row-name">{label}</span>
      <span className={`row-value long ${cls}`}>
        {value}
        {note && <small className="prop-note">{note}</small>}
      </span>
    </>
  );
  if (side)
    return (
      <div className="row prop prop-split">
        <div className="row-inner">
          <button className="prop-main" onClick={onClick}>
            {parts}
          </button>
          {side}
        </div>
      </div>
    );
  const inner = <div className="row-inner">{parts}</div>;
  return onClick ? (
    <button className="row prop" onClick={onClick}>
      {inner}
    </button>
  ) : (
    <div className="row prop">{inner}</div>
  );
}

/** `共 12 次，上次是昨天`, `共 12 次，今天已完成`. */
function completionsLabel(t: Task, now: Date): string {
  if (!t.completions) return "暂无完成记录";
  const { count, last } = t.completions;
  return `共 ${count} 次，${doneToday(t, isoDate(now)) ? "今天已完成" : `上次是${formatDay(day(last).getTime(), now)}`}`;
}

/** Files dropped on the details are added to the task; while they are dragged over, `on`. One dropped
 * in the description's editor is added there, which also refers to it. Files pasted while nothing
 * that takes text has the focus are added too. */
function useFileDrop(task: TaskId, add: (files: File[]) => unknown) {
  const [on, setOn] = useState(false);
  // Entering a child of the details leaves the one before.
  const depth = useRef(0);
  const latest = useRef(add);
  useEffect(() => {
    latest.current = add;
  });
  useEffect(() => {
    const paste = (e: ClipboardEvent) => {
      const files = filesIn(e.clipboardData);
      if (!files.length || (e.target as Element).closest("input, textarea, [contenteditable]")) return;
      e.preventDefault();
      latest.current(files);
    };
    document.addEventListener("paste", paste);
    return () => document.removeEventListener("paste", paste);
  }, [task]);
  // Not those over a dialog opened from here, which come by React's tree too: they are the quick add's.
  const ours = (e: React.DragEvent) => draggingFiles(e) && e.target instanceof Node && e.currentTarget.contains(e.target);
  return {
    on,
    handlers: {
      onDragEnter: (e: React.DragEvent) => {
        if (!ours(e)) return;
        depth.current++;
        setOn(true);
      },
      onDragLeave: (e: React.DragEvent) => {
        if (ours(e) && --depth.current === 0) setOn(false);
      },
      onDragOver: (e: React.DragEvent) => ours(e) && e.preventDefault(),
      onDrop: (e: React.DragEvent) => {
        if (!ours(e)) return;
        depth.current = 0;
        setOn(false);
        // The description's editor has taken them.
        if (e.defaultPrevented) return;
        e.preventDefault();
        latest.current(filesIn(e.dataTransfer));
      },
    },
  };
}

function Links({ board, list, onOpen, onRemove, onAdd, add }: { board: Board; list: Task[]; onOpen(t: Task): void; onRemove(t: Task): void; onAdd(): void; add: string }) {
  return (
    <div className="group">
      {list.map((o) => {
        const status = board.status(o);
        return (
          <div key={o.id} className={`row dep ${status}`} onClick={() => onOpen(o)}>
            <div className="row-inner">
              <span className={`dot ${status}`} />
              <button type="button" className="dep-main">
                <span className="dep-title">{o.title}</span>
                <span className="dep-path">{board.place(o.folder_id)}</span>
              </button>
              <Tip label="移除此依赖">
                <button
                  className="icon-btn sm"
                  onClick={(e) => {
                    e.stopPropagation();
                    onRemove(o);
                  }}
                >
                  <Icon of={X} size={16} />
                </button>
              </Tip>
            </div>
          </div>
        );
      })}
      <button className="row add-row" onClick={onAdd}>
        <div className="row-inner">
          <Icon of={Plus} />
          {add}
        </div>
      </button>
    </div>
  );
}
