// What can be done to tasks from anywhere: complete or reopen, move, delete, open or close, link.

import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { useCommit, useSession, useUndo } from "../session";
import { useFeedback } from "../ui/feedback";
import { useMarking } from "./marks";
import { useSelection } from "./nav";
import type { RowActions } from "./rows";

export function useTaskActions(): RowActions & { link(before: TaskId, after: TaskId): void; unlink(before: TaskId, after: TaskId): void } {
  const { store } = useSession();
  const commit = useCommit();
  const undo = useUndo();
  const { snack, confirm } = useFeedback();
  const marking = useMarking();
  const { selected, select, close } = useSelection();
  const board = () => store.getView().board;

  return {
    // Opening the open task again closes it.
    open: (t) => (selected === t.id ? close() : select(t.id)),

    async toggle(task: Task) {
      const t = board().task(task.id);
      if (!t) return;
      const completing = t.done_at === null;
      if (completing && board().status(t) === "waiting") {
        const ok = await confirm({ title: "前置尚未完成", body: `“${t.title}”还有 ${board().waitingOn(t)} 项前置未完成，仍要标为完成吗？`, ok: "标为完成" });
        if (!ok) return;
      }
      const readyBefore = new Set(board().next().map((x) => x.id));
      const r = commit([{ op: "task.update", id: t.id, done: completing }]);
      if (!r) return;
      if (!completing) {
        marking.pop(t.id);
        snack({ text: `已恢复“${t.title}”`, action: "撤销", onAction: () => undo(r.id) });
        return;
      }
      const unlocked = board()
        .next()
        .filter((x) => !readyBefore.has(x.id));
      marking.completed(
        t.id,
        unlocked.map((x) => x.id),
      );
      const text = unlocked.length === 1 ? `解锁了“${unlocked[0].title}”` : unlocked.length ? `解锁了 ${unlocked.length} 项` : `已完成“${t.title}”`;
      snack({ text, action: "撤销", onAction: () => undo(r.id) });
    },

    move(t: Task, folder: FolderId | null) {
      if (folder === t.folder_id) return;
      const r = commit([{ op: "task.update", id: t.id, folder_id: folder }]);
      if (r) snack({ text: `已移至“${board().folderName(folder)}”`, action: "撤销", onAction: () => undo(r.id) });
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

    unlink(before, after) {
      const r = commit([{ op: "dep.remove", before, after }]);
      if (r) snack({ text: "已移除依赖", action: "撤销", onAction: () => undo(r.id) });
    },
  };
}
