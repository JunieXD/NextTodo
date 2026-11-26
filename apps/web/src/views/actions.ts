// What can be done to tasks from anywhere: complete or reopen, move, delete, open or close, link.

import type { Due } from "../api/types/Due";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { v7 as uuidv7 } from "uuid";
import { useLocation } from "wouter";
import { lastKey } from "../domain/apply";
import { formatDue, isoDate } from "../domain/dates";
import { occurrence, shifted, skipTo } from "../domain/repeat";
import { useCommit, useSession, useUndo } from "../session";
import { useFeedback } from "../ui/feedback";
import { useWide } from "../ui/hooks";
import { useMarking } from "./marks";
import { folderUrl, useSelection } from "./nav";
import type { RowActions } from "./rows";
import { lost } from "./share";

/** “解锁了“写周报””, “解锁了 3 项”, or nothing. */
const unlocking = (unlocked: Task[]) => (unlocked.length === 1 ? `解锁了“${unlocked[0].title}”` : unlocked.length ? `解锁了 ${unlocked.length} 项` : "");

export function useTaskActions(): RowActions & {
  link(before: TaskId, after: TaskId): void;
  linkNew(task: Task, dir: "pred" | "succ", title: string, folder: FolderId | null): void;
  unlink(before: TaskId, after: TaskId): void;
} {
  const { store } = useSession();
  const commit = useCommit();
  const undo = useUndo();
  const { snack, confirm, choose } = useFeedback();
  const marking = useMarking();
  const { selected, select, close } = useSelection();
  const [location, navigate] = useLocation();
  const wide = useWide();
  const board = () => store.getView().board;

  /** Completes the task or reopens it; `endRepeat` stops a repeating one so that it is done, rather
   * than due next time. */
  const setDone = async (task: Task, endRepeat: boolean) => {
    const t = board().task(task.id);
    if (!t) return;
    const completing = t.done_at === null;
    if (completing && board().status(t) === "waiting") {
      const on = board().waitingOn(t);
      const waits = [on > 0 && `还有 ${on} 项前置未完成`, board().notStarted(t) && `要到${formatDue({ date: t.start!, time: t.start_time }, new Date())}${t.start_time ? " " : ""}才开始`,
      ];
      const ok = await confirm({
        title: on > 0 ? "前置尚未完成" : "未到开始时间",
        body: `“${t.title}”${waits.filter(Boolean).join("，")}，仍要标为完成吗？`,
        ok: "标为完成",
      });
      if (!ok) return;
    }
    const readyBefore = new Set(board().next().map((x) => x.id));
    const by = board().dueBy().get(t.id);
    // Ending the repeat keeps the rest of the due date.
    let ended: Due | null = null;
    if (endRepeat && t.due?.repeat) {
      const { repeat, ...rest } = t.due;
      ended = rest;
    }
    const repeating = completing && !ended && t.due?.repeat !== undefined;
    const r = commit([
      {
        op: "task.update",
        id: t.id,
        done: completing,
        ...(ended && { due: ended }),
        // The day here: a repeating task's next due date counts from it, as does a due date some days
        // after this is done, also once sent later. The time done is the one seen, not one after it
        // done meanwhile elsewhere.
        ...(completing && { today: isoDate(new Date()) }),
        ...(repeating && { occurrence: occurrence(t) }),
      },
    ]);
    if (!r) return;
    if (!completing) {
      marking.pop(t.id);
      snack({ text: `已恢复“${t.title}”`, action: "撤销", onAction: () => undo(r.id) });
      return;
    }
    const unlocked = board()
      .next()
      .filter((x) => !readyBefore.has(x.id));
    // Done the last time, a repeating task is done for good (docs/design.md#重复).
    const last = repeating && board().task(t.id)?.done_at !== null;
    if (repeating && !last) {
      marking.pop(t.id);
      marking.flash(unlocked.map((x) => x.id));
      const text = [`下一次：${formatDue(board().task(t.id)!.due!, new Date())}`, unlocking(unlocked)].filter(Boolean).join("，");
      snack({ text, action: "撤销", onAction: () => undo(r.id) });
      return;
    }
    marking.completed(
      t.id,
      unlocked.map((x) => x.id),
      by,
    );
    const unlocks = unlocking(unlocked);
    const text = last ? (unlocks ? `已完成最后一次，${unlocks}` : `“${t.title}”已完成最后一次`) : unlocks || `已完成“${t.title}”`;
    snack({ text, action: "撤销", onAction: () => undo(r.id) });
  };

  return {
    // Opening the open task again closes it.
    open: (t) => (selected === t.id ? close() : select(t.id)),

    /** On a computer the middle column turns to the folder, with the details as they are; on a phone
     * the folder's page takes the place of the details. The same page again is no step back. */
    reveal(t) {
      const to = folderUrl(t.folder_id);
      const url = wide && selected ? `${to}?t=${selected}` : to;
      if (location === to) navigate(url, { replace: true, state: { ...history.state, reveal: t.id } });
      else navigate(url, { replace: !wide && history.state?.sheet === true, state: { reveal: t.id } });
    },

    toggle: (t) => setDone(t, false),
    endRepeat: (t) => setDone(t, true),

    /** Gives the task up, asking first whether those after it still open go too: one change
     * (docs/design.md#放弃). */
    async drop(task) {
      const t = board().task(task.id);
      if (!t || t.done_at !== null) return;
      const after = board().openAfter(t);
      let along = false;
      if (after.length) {
        const chosen = await choose({
          title: `放弃“${t.title}”？`,
          body: `它还有 ${after.length} 项后续没结束。`,
          options: ["只放弃这一项", `连同 ${after.length} 项后续一起放弃`],
        });
        if (chosen === null) return;
        along = chosen === 1;
      }
      // As things are once answered: done meanwhile elsewhere, it or one after it is left as it is.
      const now = board().task(t.id);
      if (!now || now.done_at !== null) return;
      const going = along ? [now, ...board().openAfter(now)] : [now];
      const readyBefore = new Set(board().next().map((x) => x.id));
      const by = board().dueBy().get(t.id);
      const r = commit(going.map((x) => ({ op: "task.drop", id: x.id })));
      if (!r) return;
      const unlocked = board()
        .next()
        .filter((x) => !readyBefore.has(x.id));
      marking.completed(
        t.id,
        unlocked.map((x) => x.id),
        by,
      );
      const text = [going.length > 1 ? `已放弃 ${going.length} 项` : `已放弃“${t.title}”`, unlocking(unlocked)].filter(Boolean).join("，");
      snack({ text, action: "撤销", onAction: () => undo(r.id) });
    },

    skip(task) {
      const t = board().task(task.id);
      const due = t?.due;
      const next = due && skipTo(due, isoDate(new Date()));
      if (!t || !due || !next) return;
      const moved = { ...due, date: next };
      const readyBefore = new Set(board().next().map((x) => x.id));
      const r = commit([{ op: "task.update", id: t.id, due: moved, ...(t.start && { start: shifted(t.start, due.date, next) }) }]);
      if (!r) return;
      // Skipped while its time had begun, the tasks after it can start, as when it is done.
      const unlocked = board()
        .next()
        .filter((x) => !readyBefore.has(x.id));
      marking.flash(unlocked.map((x) => x.id));
      const text = [`已跳过，下一次：${formatDue(moved, new Date())}`, unlocking(unlocked)].filter(Boolean).join("，");
      snack({ text, action: "撤销", onAction: () => undo(r.id) });
    },

    /** Moves `t` to the end of `folder`. */
    move(t: Task, folder: FolderId | null) {
      const sort_key = lastKey(
        board()
          .contents(folder)
          .map((x) => x.sort_key),
      );
      const r = commit([{ op: "task.update", id: t.id, folder_id: folder, sort_key }]);
      if (r) snack({ text: `已移至“${board().folderName(folder)}”${lost(r.effects)}`, action: "撤销", onAction: () => undo(r.id) });
    },

    remove(t) {
      const r = commit([{ op: "task.delete", id: t.id }]);
      if (!r) return;
      if (selected === t.id) close();
      snack({ text: `已删除“${t.title}”`, action: "撤销", onAction: () => undo(r.id) });
    },

    link(before, after) {
      const r = commit([{ op: "dep.add", before, after }]);
      if (!r) return;
      const [a, b] = [board().task(before)!, board().task(after)!];
      snack({ text: `已将“${b.title}”排在“${a.title}”之后`, action: "撤销", onAction: () => undo(r.id) });
    },

    /** Creates a task in `folder`, last there, before (`pred`) or after (`succ`) `task`: one change. */
    linkNew(task, dir, title, folder) {
      const id = uuidv7();
      const [before, after] = dir === "pred" ? [id, task.id] : [task.id, id];
      const r = commit([
        { op: "task.create", id, title, sort_key: lastKey(board().contents(folder).map((t) => t.sort_key)), ...(folder && { folder_id: folder }) },
        { op: "dep.add", before, after },
      ]);
      if (r) snack({ text: `已新建${dir === "pred" ? "前置" : "后续"}“${title}”`, action: "撤销", onAction: () => undo(r.id) });
    },

    unlink(before, after) {
      const r = commit([{ op: "dep.remove", before, after }]);
      if (r) snack({ text: "已移除依赖", action: "撤销", onAction: () => undo(r.id) });
    },
  };
}
