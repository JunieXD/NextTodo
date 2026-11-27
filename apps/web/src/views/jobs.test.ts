import { expect, test } from "vitest";
import type { AiJob } from "../api/types/AiJob";
import { apply, fromSnapshot, toSnapshot } from "../domain/apply";
import { Board } from "../domain/board";
import { originalTaskOps } from "./jobs";

const me = "019a4b2c-0000-7000-8000-000000000001";
const jobId = "019a4b2c-0000-7000-8000-000000000002";
const taskId = "019a4b2c-0000-7000-8000-000000000003";

test.each([
  { day: "2026-11-30", time: "13:00", due: { date: "2026-11-30", time: "13:00", reminders: [{ before: 30 }] } },
  { day: "2026-11-30", due: { date: "2026-11-30", reminders: [{ days: 1, time: "09:00" }] } },
  { due: null },
])("adding the original text uses the job’s saved calendar slot, if any: $due", ({ due, ...calendar }) => {
  const job: AiJob = {
    id: jobId,
    folder_id: null,
    kind: "organize",
    text: "明天寄发票",
    local_time: "2026-11-04 16:30 星期三",
    state: "failed",
    error: "接口超时",
    suggestions: null,
    reminders: { timed: [{ before: 60 }], dated: [] },
    created_at: 0,
    updated_at: 0,
    ...calendar,
  };
  const snapshot = { folders: [], tasks: [], dependencies: [], completions: [], attachments: [], ai_jobs: [job], shares: [], people: [], prefs: { reminders: { timed: [{ before: 30 }], dated: [{ days: 1, time: "09:00" }] } } };
  const local = { today: "2026-11-05" };
  const board = new Board(snapshot, local, me);
  const data = fromSnapshot(snapshot);
  apply(data, originalTaskOps(job, board, taskId).ops, { me, now: 0, local, effectsOf: () => undefined, effectsAfter: () => [] });
  const after = toSnapshot(data);
  expect(after.tasks.map((task) => ({ title: task.title, due: task.due }))).toEqual([{ title: job.text, due }]);
  expect(after.ai_jobs).toEqual([]);
});
