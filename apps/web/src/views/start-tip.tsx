// The tip that a start time keeps a repeating task out of “接下来” until near its due date
// (docs/design/dates.md#开始时间): over “接下来” while a repeating task there without one, done for today,
// waits for its next time. Gone for good on this device, for each user, once a button on it is pressed
// or a start is set here.

import { useState, useSyncExternalStore } from "react";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { isoDate } from "../domain/dates";
import { doneToday } from "../domain/repeat";
import { useCommit, useView } from "../session";
import { StartEditor } from "./due";

const PREFIX = "nexttodo.startTipSeen:";
const listeners = new Set<() => void>();

function subscribe(l: () => void) {
  listeners.add(l);
  addEventListener("storage", l);
  return () => {
    listeners.delete(l);
    removeEventListener("storage", l);
  };
}

export function startTipSeen(user: string) {
  localStorage.setItem(PREFIX + user, "");
  for (const l of listeners) l();
}

/** The tip about the first of the `ready` tasks it fits, and the start time panel it opens. */
export function StartTip({ ready, now }: { ready: Task[]; now: Date }) {
  const { board } = useView();
  const commit = useCommit();
  const seen = useSyncExternalStore(subscribe, () => localStorage.getItem(PREFIX + board.me) !== null);
  const [editing, setEditing] = useState<TaskId | null>(null);
  const today = isoDate(now);
  const t = seen ? undefined : ready.find((x) => x.due?.repeat && !x.start && doneToday(x, today));
  const task = editing === null ? undefined : board.task(editing);
  return (
    <>
      {t && (
        <div className="tip-card">
          <p>“{t.title}”完成后会留在这里，等待下一次。如不想提前看到，可设为截止前几天才显示。</p>
          <div className="tip-actions">
            <button className="text-btn" onClick={() => startTipSeen(board.me)}>
              知道了
            </button>
            <button className="text-btn" onClick={() => (startTipSeen(board.me), setEditing(t.id))}>
              设开始时间
            </button>
          </div>
        </div>
      )}
      {task && (
        <StartEditor
          start={task.start}
          time={task.start_time}
          due={task.due}
          dueAfter={task.due_after}
          onSave={(s) => commit([{ op: "task.update", id: task.id, ...(s ? { start: s.date, start_time: s.time ?? null } : { start: null }) }])}
          onClose={() => setEditing(null)}
        />
      )}
    </>
  );
}
