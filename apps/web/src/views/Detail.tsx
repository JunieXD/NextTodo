// A task's details: title, folder, due date, description, what it waits for and what waits for it.

import { Bell, Calendar, Folder as FolderIcon, KeyRound, type LucideIcon, Plus, User, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { formatDue, formatMoment, remindLabel } from "../domain/dates";
import { checkDescription, checkTitle } from "../domain/text";
import { useCommit, useSession, useUndo, useView } from "../session";
import { Check, Seg } from "../ui/controls";
import { useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { Description } from "../ui/Markdown";
import { useTaskActions } from "./actions";
import { useMarks } from "./marks";
import { useSelection } from "./nav";
import { DueEditor, FolderPicker, TaskPicker } from "./pickers";
import { dueClass } from "./rows";

const STATUS = { ready: "可做", waiting: "等待中", done: "已完成" };

type Panel = "folder" | "due" | "pred" | "succ" | null;

/** Who made a change: nobody named for the user, or a token's name followed by a space before the Chinese. */
const who = (via: string | null) => (via ? `${via} ` : "");

type TaskFields = Omit<Extract<Op, { op: "task.update" }>, "op" | "id">;

/** Saves a text field; what `useCommit` gives back. */
type Save = (value: string) => { id: MutationId } | null;

/** An edit in progress and the text it started from. */
type Draft = { text: string; base: string };

/** What an edit left in its field, and whether that is the edit. */
type Kept = { value: string; mine: boolean };

/** Saves `mine`, edited from `base`, into a field of a task. When the field has changed elsewhere
 * meanwhile, asks whether to keep that or overwrite it. Overwriting can be undone, while giving up the
 * edit cannot, so closing the dialog overwrites; it overwrites whatever is there by the answer, which
 * undoing brings back. Gives what the field holds afterwards; nothing when the task is gone. */
function useKeepEdit(field: "title" | "description", what: string) {
  const { store } = useSession();
  const { confirm, snack } = useFeedback();
  const undo = useUndo();
  return async (id: TaskId, base: string, mine: string, save: Save): Promise<Kept | null> => {
    const current = () => store.getView().board.task(id);
    let t = current();
    if (!t) return null;
    // Nothing edited: what is there stays, whoever wrote it.
    if (mine === base) return { value: t[field], mine: false };
    if (t[field] !== base && t[field] !== mine) {
      const theirs = await confirm({
        title: `${what}在你编辑时${t.updated_via ? `被“${t.updated_via}”` : "在别处被"}改过`,
        body: "对方现在的版本如下。保留对方的，你这次的修改就会丢掉。",
        detail: t[field],
        ok: "保留对方的",
        cancel: "用我的覆盖",
        danger: true,
      });
      t = current();
      if (!t) return null;
      if (theirs) return { value: t[field], mine: false };
      if (t[field] !== mine) {
        const r = save(mine);
        if (r) snack({ text: `已用你的版本覆盖${what}`, action: "撤销", onAction: () => undo(r.id) });
      }
      return { value: mine, mine: true };
    }
    if (t[field] !== mine) save(mine);
    return { value: mine, mine: true };
  };
}

/** `closing`: the desktop pane is sliding away with this task still in it. Its panels close at once,
 * so their focus trap does not hold on to focus that has moved on, such as to a quick add. */
export function Detail({ id, now, closing, onClose }: { id: TaskId; now: Date; closing?: boolean; onClose?: () => void }) {
  const { board } = useView();
  const t = board.task(id);
  const actions = useTaskActions();
  const commit = useCommit();
  const { pop } = useMarks();
  const { select } = useSelection();
  const [panel, setPanel] = useState<Panel>(null);
  if (closing && panel) setPanel(null);
  if (!t) return <div className="detail-empty">该事项已删除</div>;
  const status = board.status(t);
  const update = (fields: TaskFields) => commit([{ op: "task.update", id: t.id, ...fields }]);

  return (
    <div className="detail">
      <div className="d-head" data-drag>
        <Check status={status} large pop={pop === t.id} onToggle={() => actions.toggle(t)} />
        <div className="d-title-wrap">
          <Title key={t.id} task={t} onSave={(title) => update({ title })} />
          <div className="d-status">
            <span className={`pill ${status}`}>{STATUS[status]}</span>
            {status === "waiting" && <span>还有 {board.waitingOn(t)} 项前置未完成</span>}
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
        <Prop icon={FolderIcon} label="文件夹" value={board.place(t.folder_id)} onClick={() => setPanel("folder")} />
        <Prop icon={Calendar} label="截止" value={t.due ? formatDue(t.due, now) : "未设置"} cls={t.due ? dueClass(t, now) : "muted"} onClick={() => setPanel("due")} />
        {t.due?.time && (
          <Prop icon={Bell} label="提醒" value={remindLabel(t.due.remind_before)} cls={t.due.remind_before === undefined ? "muted" : ""} onClick={() => setPanel("due")} />
        )}
      </div>

      <DescriptionSection key={t.id} task={t} onSave={(description) => update({ description })} />

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
          title="移动到"
          current={t.folder_id}
          onPick={(f) => actions.move(t, f)}
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
function Title({ task, onSave }: { task: Task; onSave: Save }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const { snack } = useFeedback();
  const keep = useKeepEdit("title", "标题");
  if (draft === null) {
    return (
      <h2 className="d-title">
        <button type="button" onClick={() => setDraft({ text: task.title, base: task.title })}>
          {task.title}
        </button>
      </h2>
    );
  }
  const save = () => {
    setDraft(null);
    const c = checkTitle(draft.text);
    if (!c.ok) snack(c.error);
    else void keep(task.id, draft.base, c.value, onSave);
  };
  return (
    <textarea
      className="title-edit"
      autoFocus
      rows={1}
      value={draft.text}
      aria-label="标题"
      onChange={(e) => setDraft({ ...draft, text: e.target.value.replace(/\n/g, "") })}
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
function DescriptionSection({ task, onSave }: { task: Task; onSave: Save }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const { snack } = useFeedback();
  const keep = useKeepEdit("description", "描述");
  const pending = useRef({ draft, task, onSave });
  useEffect(() => {
    pending.current = { draft, task, onSave };
  });
  /** The save waiting for an answer, if any: leaving meanwhile does not save, or ask, a second time. */
  const saving = useRef<Promise<Kept | null>>(undefined);
  /** Saves the draft; gives what the description holds afterwards, or nothing when it was not saved. */
  const save = (d: Draft | null, t: Task, done: Save) => {
    if (d === null || saving.current) return saving.current;
    const c = checkDescription(d.text);
    if (!c.ok) return void snack(c.error);
    saving.current = keep(t.id, d.base, c.value, done).finally(() => (saving.current = undefined));
    return saving.current;
  };
  useEffect(
    () => () => {
      const { draft, task, onSave } = pending.current;
      void save(draft, task, onSave);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps -- saves once, when leaving
    [],
  );
  const mode = (v: "view" | "edit") => {
    if (v === "edit") setDraft({ text: task.description, base: task.description });
    else {
      void save(draft, task, onSave);
      setDraft(null);
    }
  };
  /** Saves on leaving the box while still editing: what is saved, or kept instead, is the base from then on. */
  const blur = async () => {
    const kept = await save(draft, task, onSave);
    if (kept) setDraft((d) => d && (kept.mine ? { ...d, base: kept.value } : { text: kept.value, base: kept.value }));
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
            value={draft.text}
            placeholder="支持 Markdown，以 - [ ] 开头为清单项"
            onChange={(e) => setDraft({ ...draft, text: e.target.value })}
            onBlur={() => void blur()}
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
              <button type="button" className="dep-main">
                <span className="dep-title">{o.title}</span>
                <span className="dep-path">{board.place(o.folder_id)}</span>
              </button>
              <button
                className="icon-btn sm"
                aria-label="移除此依赖"
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
