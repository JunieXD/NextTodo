// Copying a folder with all in it, to use a flow again (docs/design.md#文件夹).

import { v7 as uuidv7 } from "uuid";
import type { Attachment } from "../api/types/Attachment";
import type { Folder } from "../api/types/Folder";
import type { FolderId } from "../api/types/FolderId";
import type { Op } from "../api/types/Op";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "./board";
import { MAX_NAME } from "./text";

/** What a copy of a folder named `name` is called where `taken` says which names are in use: its own
 * name, else "名字 副本", "名字 副本 2" and on. As `copy_name` in crates/server/src/tools/write.rs. */
export function copyName(name: string, taken: (n: string) => boolean): string {
  // A long name is cut short to leave room for what tells the copy apart.
  const copy = (mark: string) => [...name].slice(0, MAX_NAME - [...mark].length).join("") + mark;
  for (let n = 0; ; n++) {
    const candidate = n === 0 ? name : copy(n === 1 ? " 副本" : ` 副本 ${n}`);
    if (!taken(candidate)) return candidate;
  }
}

/** Where a copy of `f` goes unless said: beside it, or, shared with the user, at their root. */
export const copyPlace = (board: Board, f: FolderId): FolderId | null =>
  board.sharedWithMe().some((s) => s.id === f) ? null : board.parent(f);

/** The changes that copy folder `f` into `parent` as folder `root`, named `name`, at `sort_key`
 * there: its subfolders, its tasks open again, the dependencies between them, and their attachments;
 * and the attachments whose files are to be copied, each with the one it copies. As `copy` in
 * crates/server/src/tools/write.rs. */
export function copyFolder(
  board: Board,
  f: FolderId,
  parent: FolderId | null,
  root: { id: FolderId; name: string; sort_key: string },
): { ops: Op[]; files: [string, Attachment][] } {
  const look = ({ color, icon }: Folder) => ({ ...(color && { color }), ...(icon && { icon }) });
  const ops: Op[] = [{ op: "folder.create", ...root, ...look(board.folder(f)!), ...(parent && { parent_id: parent }) }];
  const tasks: [Task, TaskId, FolderId][] = [];
  const stack: [FolderId, FolderId][] = [[f, root.id]];
  while (stack.length) {
    const [old, folder] = stack.pop()!;
    tasks.push(...board.contents(old).map((t): [Task, TaskId, FolderId] => [t, uuidv7(), folder]));
    for (const sub of board.subfolders(old)) {
      const id = uuidv7();
      ops.push({ op: "folder.create", id, name: sub.name, parent_id: folder, sort_key: sub.sort_key, ...look(sub) });
      stack.push([sub.id, id]);
    }
  }
  const copied = new Map(tasks.map(([t, id]) => [t.id, id]));
  const files: [string, Attachment][] = [];
  const adds: Op[] = [];
  const deps: Op[] = [];
  for (const [t, id, folder] of tasks) {
    let description = t.description;
    for (const a of board.attachments(t.id)) {
      const copy = uuidv7();
      files.push([copy, a]);
      adds.push({ op: "attachment.add", id: copy, name: a.name, type: a.type, size: a.size, task_id: id });
      description = description.replaceAll(`attachment:${a.id}`, `attachment:${copy}`);
    }
    ops.push({
      op: "task.create",
      id,
      title: t.title,
      folder_id: folder,
      ...(description && { description }),
      ...(t.start && { start: t.start }),
      ...(t.start_time && { start_time: t.start_time }),
      ...(t.due && { due: t.due }),
      sort_key: t.sort_key,
      // Still in the shared folder it was in, a task keeps who it is assigned to.
      ...(board.sharedRoot(t.folder_id) === board.sharedRoot(parent) && t.assignees.length && { assignees: t.assignees }),
    });
    for (const after of board.after(t)) {
      const to = copied.get(after.id);
      if (to) deps.push({ op: "dep.add", before: id, after: to });
    }
  }
  return { ops: [...ops, ...deps, ...adds], files };
}
