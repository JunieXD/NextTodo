// A task's details: title, folder, due date, description, what it waits for and what waits for it.

import { Bell, Calendar, Folder as FolderIcon, KeyRound, type LucideIcon, Plus, User, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { formatDue, formatMoment, remindLabel } from "../domain/dates";
import { checkDescription, checkTitle } from "../domain/text";
import { useCommit, useView } from "../session";
import { Check, Seg } from "../ui/controls";
import { useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { Description } from "../ui/Markdown";
import { useTaskActions } from "./actions";
import { useMarks } from "./marks";
import { useSelection } from "./nav";
import { DueEditor, FolderPicker, TaskPicker } from "./pickers";
import { dueClass, place } from "./rows";

const STATUS = { ready: "可做", waiting: "等待中", done: "已完成" };

type Panel = "folder" | "due" | "pred" | "succ" | null;

/** Who made a change: `你`, or a token's name followed by a space before the Chinese. */
const who = (via: string | null) => (via ? `${via} ` : "你");

type TaskFields = Omit<Extract<Op, { op: "task.update" }>, "op" | "id">;

export function Detail({ id, now, onClose }: { id: TaskId; now: Date; onClose?: () => void }) {
  const { board } = useView();
  const t = board.task(id);
  const actions = useTaskActions();
  const commit = useCommit();
  const { snack } = useFeedback();
  const { pop } = useMarks();
  const { select } = useSelection();
  const [panel, setPanel] = useState<Panel>(null);
  if (!t) return <div className="detail-empty">这一项已经删除了</div>;
  const status = board.status(t);
  const update = (fields: TaskFields) => commit([{ op: "task.update", id: t.id, ...fields }]);
  const move = (f: string | null) => {
    if (f !== t.folder_id && update({ folder_id: f })) snack(`已移到“${f ? board.folder(f)!.name : "根目录"}”`);
  };

  return (
    <div className="detail">
      <div className="d-head" data-drag>
        <Check status={status} large pop={pop === t.id} onToggle={() => actions.toggle(t)} />
        <div className="d-title-wrap">
          <Title key={t.id} task={t} onSave={(title) => update({ title })} />
          <div className="d-status">
            <span className={`pill ${status}`}>{STATUS[status]}</span>
            {status === "waiting" && <span>还差 {board.waitingOn(t)} 项前置</span>}
            {t.number > 0 && <span className="tabular-nums">#{t.number}</span>}
          </div>
        </div>
        {onClose && (
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            <Icon of={X} />
          </button>
        )}
      </div>

      <div className="group">
        <Prop icon={FolderIcon} label="文件夹" value={place(board, t.folder_id)} onClick={() => setPanel("folder")} />
        <Prop icon={Calendar} label="截止" value={t.due ? formatDue(t.due, now) : "未设置"} cls={t.due ? dueClass(t, now) : "muted"} onClick={() => setPanel("due")} />
        {t.due?.time && (
          <Prop icon={Bell} label="提醒" value={remindLabel(t.due.remind_before)} cls={t.due.remind_before === undefined ? "muted" : ""} onClick={() => setPanel("due")} />
        )}
      </div>

      <DescriptionSection key={t.id} task={t} onSave={(description) => update({ description })} />

      <div className="d-sec">
        <div>
          <h3>前置</h3>
          <p>这些做完，才能开始这一项</p>
        </div>
      </div>
      <Links board={board} list={board.before(t)} onOpen={(o) => select(o.id)} onRemove={(o) => actions.unlink(o.id, t.id)} onAdd={() => setPanel("pred")} add="添加前置" />
      <div className="d-sec">
        <div>
          <h3>后续</h3>
          <p>这一项做完，才能开始这些</p>
        </div>
      </div>
      <Links board={board} list={board.after(t)} onOpen={(o) => select(o.id)} onRemove={(o) => actions.unlink(t.id, o.id)} onAdd={() => setPanel("succ")} add="添加后续" />

      <p className="provenance">
        <Icon of={t.created_via ? KeyRound : User} size={14} />
        {who(t.created_via)}创建于 {formatMoment(t.created_at, now)}
        {t.updated_at !== t.created_at && `，${who(t.updated_via)}修改于 ${formatMoment(t.updated_at, now)}`}
      </p>
      <div className="group danger detail-actions">
        <button className="row" onClick={() => actions.remove(t)}>
          <div className="row-inner">删除事项</div>
        </button>
      </div>

      {panel === "folder" && (
        <FolderPicker
          title="移到文件夹"
          current={t.folder_id}
          onPick={move}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "due" && <DueEditor due={t.due} onSave={(due) => update({ due })} onClose={() => setPanel(null)} />}
      {(panel === "pred" || panel === "succ") && (
        <TaskPicker
          task={t}
          dir={panel}
          onPick={(o) => (panel === "pred" ? actions.link(o.id, t.id) : actions.link(t.id, o.id))}
          onClose={() => setPanel(null)}
        />
      )}
    </div>
  );
}

function Prop({ icon, label, value, cls = "", onClick }: { icon: LucideIcon; label: string; value: string; cls?: string; onClick(): void }) {
  return (
    <button className="row prop" onClick={onClick}>
      <div className="row-inner">
        <Icon of={icon} />
        <span>{label}</span>
        <span className={`value ${cls}`}>{value}</span>
      </div>
    </button>
  );
}

/** The title, edited in place. */
function Title({ task, onSave }: { task: Task; onSave(title: string): void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const { snack } = useFeedback();
  if (draft === null) {
    return (
      <h2 className="d-title" onClick={() => setDraft(task.title)}>
        {task.title}
      </h2>
    );
  }
  const save = () => {
    setDraft(null);
    const c = checkTitle(draft);
    if (!c.ok) snack(c.error);
    else if (c.value !== task.title) onSave(c.value);
  };
  return (
    <textarea
      className="title-edit"
      autoFocus
      rows={1}
      value={draft}
      aria-label="标题"
      onChange={(e) => setDraft(e.target.value.replace(/\n/g, ""))}
      onBlur={save}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !e.nativeEvent.isComposing) e.currentTarget.blur();
        if (e.key === "Escape") setDraft(null);
      }}
    />
  );
}

/** The description: shown as Markdown with tickable checklists, or edited as text. An edit is saved
 * when leaving the editor, also by switching to another task. */
function DescriptionSection({ task, onSave }: { task: Task; onSave(description: string): void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const { snack } = useFeedback();
  const pending = useRef<{ draft: string | null; task: Task; onSave(d: string): void }>({ draft, task, onSave });
  useEffect(() => {
    pending.current = { draft, task, onSave };
  });
  const save = (text: string | null, t: Task, done: (d: string) => void) => {
    if (text === null || text === t.description) return;
    const c = checkDescription(text);
    if (c.ok) done(c.value);
    else snack(c.error);
  };
  useEffect(
    () => () => {
      const { draft, task, onSave } = pending.current;
      save(draft, task, onSave);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps -- saves once, when leaving
    [],
  );
  const mode = (v: "view" | "edit") => {
    if (v === "edit") setDraft(task.description);
    else {
      save(draft, task, onSave);
      setDraft(null);
    }
  };
  return (
    <>
      <div className="d-sec">
        <h3>描述</h3>
        <Seg
          small
          items={[
            ["view", "预览"],
            ["edit", "编辑"],
          ]}
          value={draft === null ? "view" : "edit"}
          onChange={mode}
        />
      </div>
      <div className="group">
        {draft === null ? (
          <div className="md">
            <Description source={task.description} onChange={onSave} />
          </div>
        ) : (
          <textarea
            className="md-edit"
            value={draft}
            placeholder="支持 Markdown，- [ ] 开头是清单"
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => save(draft, task, onSave)}
          />
        )}
      </div>
    </>
  );
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
              <span className="dep-main">
                <span className="dep-title">{o.title}</span>
                <span className="dep-path">{place(board, o.folder_id)}</span>
              </span>
              <button
                className="icon-btn sm"
                aria-label="移除这条依赖"
                onClick={(e) => {
                  e.stopPropagation();
                  onRemove(o);
                }}
              >
                <Icon of={X} size={16} />
              </button>
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
