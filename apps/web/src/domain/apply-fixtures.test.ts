// The rules of changes as `fixtures/apply.json` has them; the server is checked against the same
// cases (crates/server/tests/apply.rs). Names stand for ids, and things are called by their ids.

import { expect, test } from "vitest";
import type { JobState } from "../api/types/JobState";
import type { Op } from "../api/types/Op";
import cases from "../../../../fixtures/apply.json";
import { type Data, Rejected, apply, fromSnapshot, lastKeys } from "./apply";

type Case = {
  name: string;
  me?: string;
  folders?: [string, string | null][];
  shares?: [string, string, string[]][];
  tasks?: ([string, string | null] | [string, string | null, string[]])[];
  deps?: [string, string][];
  attachments?: [string, string][];
  jobs?: [string, string | null, JobState][];
  /** As the API takes them (docs/api.md#改动). */
  ops: Op[];
} & ({ error: { code: string; message: string } } | { expect: Partial<Seen> });

/** The data as the user sees it, made as the case says. */
function given(c: Case): Data {
  const keys = lastKeys([], Math.max(c.folders?.length ?? 0, c.tasks?.length ?? 0));
  const jobs = new Set(c.jobs?.map(([id]) => id));
  return fromSnapshot({
    folders: (c.folders ?? []).map(([id, parent_id], i) => ({ id, parent_id, name: id, sort_key: keys[i], color: null, icon: null, created_at: 0, updated_at: 0 })),
    tasks: (c.tasks ?? []).map(([id, folder_id, assignees = []], i) => ({
      id,
      number: i + 1,
      folder_id,
      title: id,
      description: "",
      done_at: null,
      start: null,
      due: null,
      sort_key: keys[i],
      assignees: assignees.toSorted(),
      created_at: 0,
      updated_at: 0,
      created_by: c.me ?? "alice",
      updated_by: c.me ?? "alice",
      created_via: null,
      updated_via: null,
    })),
    dependencies: (c.deps ?? []).map(([before, after]) => ({ before, after, created_at: 0 })),
    completions: [],
    attachments: (c.attachments ?? []).map(([id, of]) => ({
      id,
      task_id: jobs.has(of) ? null : of,
      job_id: jobs.has(of) ? of : null,
      name: `${id}.txt`,
      type: "text/plain",
      size: 1,
      created_at: 0,
    })),
    ai_jobs: (c.jobs ?? []).map(([id, folder_id, state]) => ({
      id,
      folder_id,
      kind: "organize",
      text: "整理",
      local_time: "2026-11-23 10:00 星期一",
      state,
      error: state === "failed" ? "模型接口出错" : null,
      suggestions: null,
      created_at: 0,
      updated_at: 0,
    })),
    shares: (c.shares ?? []).map(([folder_id, owner, members]) => ({ folder_id, owner, members: members.toSorted() })),
    people: ["alice", "bob", "carol"].map((id) => ({ id, username: id, display_name: id })),
  });
}

type Seen = ReturnType<typeof seen>;

/** What the case looks at of the data. */
const seen = (d: Data) => ({
  folders: Object.fromEntries([...d.folders.values()].map((f) => [f.id, f.parent_id])),
  tasks: Object.fromEntries([...d.tasks.values()].map((t) => [t.id, t.folder_id])),
  deps: [...d.deps.values()].map((dep) => [dep.before, dep.after]),
  assignees: Object.fromEntries([...d.tasks.values()].filter((t) => t.assignees.length).map((t) => [t.id, t.assignees])),
  attachments: Object.fromEntries([...d.attachments.values()].map((a) => [a.id, a.task_id ?? a.job_id])),
  jobs: Object.fromEntries([...d.jobs.values()].map((j) => [j.id, j.state])),
});

test.each(cases as Case[])("changes go as the fixtures say: $name", (c) => {
  const d = given(c);
  const before = structuredClone(d);
  const run = () => apply(d, c.ops, { me: c.me ?? "alice", now: 1000, effectsOf: () => undefined, effectsAfter: () => [] });
  if ("error" in c) {
    let thrown: unknown;
    try {
      run();
    } catch (e) {
      thrown = e;
    }
    // A `Rejected` is what the outbox holds back as stuck, rather than breaking the view.
    expect(thrown).toBeInstanceOf(Rejected);
    expect(thrown).toMatchObject(c.error);
    // Turned down, the batch leaves nothing behind, what came before the refused operation included.
    expect(d).toEqual(before);
    return;
  }
  expect(Object.keys(c.expect)).not.toHaveLength(0);
  run();
  const now = seen(d);
  for (const what of Object.keys(c.expect) as (keyof Seen)[]) {
    const sorted = (v: unknown) => (what === "deps" ? (v as string[][]).toSorted() : v);
    expect(sorted(now[what]), what).toEqual(sorted(c.expect[what]));
  }
});
