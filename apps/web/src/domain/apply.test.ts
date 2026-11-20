import { expect, test } from "vitest";
import type { Effect } from "../api/types/Effect";
import type { Op } from "../api/types/Op";
import type { Repeat } from "../api/types/Repeat";
import cases from "../../../../fixtures/undo-done.json";
import { type Data, Rejected, apply, fromSnapshot, revert, stillLast } from "./apply";

test.each(cases)("a time done is still the last as the fixtures say: $name", ({ done, later, last }) => {
  expect(stillLast(done[0], done[1], later as [number, number][])).toBe(last);
});

/** The user whose data it is. */
const ME = "019a4b2c-0000-7000-8000-0000000000ee";

const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const log = new Map<string, Effect[]>();
const cx = {
  me: ME,
  now: 1000,
  effectsOf: (m: string) => log.get(m),
  effectsAfter: (m: string) => [...log.entries()].slice([...log.keys()].indexOf(m) + 1).map(([, e]) => e),
};

function empty(): Data {
  return fromSnapshot({ folders: [], tasks: [], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] });
}

/** Applies a mutation and remembers its effects under `mutation`, for undo. */
function run(d: Data, mutation: number, ops: Op[]): Effect[] {
  const effects = apply(d, ops, cx);
  log.set(id(mutation), effects);
  return effects;
}

const titles = (d: Data) => [...d.tasks.values()].map((t) => t.title);

test("new things go last, and names stay unique among siblings", () => {
  const d = empty();
  run(d, 100, [
    { op: "folder.create", id: id(1), name: "工作" },
    { op: "task.create", id: id(2), title: "一", folder_id: id(1) },
    { op: "task.create", id: id(3), title: "二", folder_id: id(1) },
  ]);
  const [a, b] = [d.tasks.get(id(2))!, d.tasks.get(id(3))!];
  expect(a.sort_key < b.sort_key).toBe(true);
  expect(a.number).toBe(0);
  expect(() => apply(d, [{ op: "folder.create", id: id(4), name: "工作" }], cx)).toThrow("同一层级已有名为“工作”的文件夹");
});

test("a failed mutation leaves nothing behind", () => {
  const d = empty();
  run(d, 101, [
    { op: "task.create", id: id(1), title: "设计" },
    { op: "task.create", id: id(2), title: "开发" },
    { op: "dep.add", before: id(1), after: id(2) },
  ]);
  const ops: Op[] = [
    { op: "task.create", id: id(3), title: "测试" },
    { op: "dep.add", before: id(2), after: id(1) },
  ];
  expect(() => apply(d, ops, cx)).toThrow(new Rejected("cycle", "添加该依赖将形成循环：“开发” → “设计” → “开发”"));
  expect(titles(d)).toEqual(["设计", "开发"]);
  expect(d.deps.size).toBe(1);
});

test("deleting a folder takes everything inside, and undo brings it back", () => {
  const d = empty();
  run(d, 102, [
    { op: "folder.create", id: id(1), name: "论文" },
    { op: "folder.create", id: id(2), name: "答辩", parent_id: id(1) },
    { op: "task.create", id: id(3), title: "PPT", folder_id: id(2) },
    { op: "task.create", id: id(4), title: "外面" },
    { op: "dep.add", before: id(4), after: id(3) },
  ]);
  const effects = run(d, 103, [{ op: "folder.delete", id: id(1) }]);
  expect(effects.map((e) => e.kind)).toEqual(["dependency", "task", "folder", "folder"]);
  expect(effects.slice(2).map((e) => e.before && "name" in e.before && e.before.name)).toEqual(["答辩", "论文"]);
  expect([d.folders.size, titles(d), d.deps.size]).toEqual([0, ["外面"], 0]);

  run(d, 104, [{ op: "undo", mutation: id(103) }]);
  expect([d.folders.size, titles(d).sort(), d.deps.size]).toEqual([2, ["PPT", "外面"], 1]);
});

test("attachments go with their task, folder or AI job, and come back with undo", () => {
  const d = empty();
  const add = (attachment: number, holder: { task_id: string } | { job_id: string }): Op => ({
    op: "attachment.add",
    id: id(attachment),
    name: `图片 ${attachment}.png`,
    type: "image/png",
    size: 100,
    ...holder,
  });
  run(d, 120, [
    { op: "folder.create", id: id(1), name: "报销" },
    { op: "task.create", id: id(2), title: "交发票", folder_id: id(1) },
    add(3, { task_id: id(2) }),
    { op: "ai_job.create", id: id(4), text: "整理这些票", local_time: "2026-11-17 09:00 星期二" },
    add(5, { job_id: id(4) }),
  ]);
  expect(() => apply(d, [add(6, { task_id: id(99) })], cx)).toThrow("事项不存在");
  expect(() => apply(d, [{ op: "attachment.add", id: id(6), name: "a", type: "image/png", size: 1 }], cx)).toThrow(Rejected);

  run(d, 121, [{ op: "attachment.update", id: id(3), name: "发票.png" }]);
  const effects = run(d, 122, [{ op: "folder.delete", id: id(1) }]);
  expect(effects.map((e) => e.kind)).toEqual(["attachment", "task", "folder"]);
  run(d, 123, [{ op: "ai_job.delete", id: id(4) }]);
  expect(d.attachments.size).toBe(0);

  run(d, 124, [{ op: "undo", mutation: id(122) }]);
  run(d, 125, [{ op: "undo", mutation: id(123) }]);
  expect([...d.attachments.values()].map((a) => [a.name, a.task_id, a.job_id])).toEqual([
    ["发票.png", id(2), null],
    ["图片 5.png", null, id(4)],
  ]);
  // Moved from the job to a task, as adding it as it was written does, and back.
  run(d, 126, [{ op: "attachment.update", id: id(5), task_id: id(2) }]);
  expect(d.attachments.get(id(5))!.job_id).toBe(null);
  run(d, 127, [{ op: "undo", mutation: id(126) }]);
  expect([d.attachments.get(id(5))!.task_id, d.attachments.get(id(5))!.job_id]).toEqual([null, id(4)]);
  run(d, 128, [{ op: "undo", mutation: id(121) }]);
  expect(d.attachments.get(id(3))!.name).toBe("图片 3.png");
  // Moved twice and undone first to last, it belongs to one thing at every step.
  run(d, 129, [{ op: "attachment.update", id: id(5), task_id: id(2) }]);
  run(d, 130, [{ op: "task.create", id: id(6), title: "寄发票" }]);
  run(d, 131, [{ op: "attachment.update", id: id(5), task_id: id(6) }]);
  run(d, 132, [{ op: "undo", mutation: id(129) }]);
  run(d, 133, [{ op: "undo", mutation: id(131) }]);
  expect([d.attachments.get(id(5))!.task_id, d.attachments.get(id(5))!.job_id]).toEqual([id(2), null]);
});

test("undo puts back only the fields the change touched", () => {
  const d = empty();
  run(d, 105, [{ op: "task.create", id: id(1), title: "初稿" }]);
  run(d, 106, [{ op: "task.update", id: id(1), done: true }]);
  run(d, 107, [{ op: "task.update", id: id(1), title: "终稿" }]);
  run(d, 108, [{ op: "undo", mutation: id(106) }]);
  const t = d.tasks.get(id(1))!;
  expect([t.title, t.done_at]).toEqual(["终稿", null]);
  expect(() => apply(d, [{ op: "undo", mutation: id(999) }], cx)).toThrow("要撤销的改动不存在或已过期");
});

test("folders cannot move into themselves", () => {
  const d = empty();
  run(d, 109, [
    { op: "folder.create", id: id(1), name: "上" },
    { op: "folder.create", id: id(2), name: "下", parent_id: id(1) },
  ]);
  expect(() => apply(d, [{ op: "folder.update", id: id(1), parent_id: id(2) }], cx)).toThrow("文件夹不能移入自身或其子文件夹");
});

test("a repeating task moves on rather than being done, and is never done while it repeats", () => {
  const d = empty();
  const weekly = { every: 1, unit: "week", from: "due" } as const;
  run(d, 120, [{ op: "task.create", id: id(1), title: "开例会", due: { date: "2026-11-09", time: "10:00", repeat: weekly } }]);
  expect(() => apply(d, [{ op: "task.update", id: id(1), done: true }], cx)).toThrow("完成重复的事项需要给出今天的日期");
  run(d, 121, [{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  expect(d.tasks.get(id(1))).toMatchObject({ done_at: null, due: { date: "2026-11-16", time: "10:00", repeat: weekly }, completions: { count: 1, last: "2026-11-11" } });
  // Undone after the reminder changed, it moves back and the reminder stays as changed; it was never done.
  run(d, 122, [{ op: "task.update", id: id(1), due: { date: "2026-11-16", time: "10:00", remind_before: 30, repeat: weekly } }]);
  run(d, 123, [{ op: "undo", mutation: id(121) }]);
  expect(d.tasks.get(id(1))!.due).toEqual({ date: "2026-11-09", time: "10:00", remind_before: 30, repeat: weekly });
  expect(d.tasks.get(id(1))!.completions).toBeUndefined();
  // Undone after the due date was cleared, only the date comes back: the rest stays cleared.
  run(d, 124, [{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  run(d, 125, [{ op: "task.update", id: id(1), due: null }]);
  run(d, 126, [{ op: "undo", mutation: id(124) }]);
  expect(d.tasks.get(id(1))!.due).toEqual({ date: "2026-11-09" });
  // Repeating no more as it is done, it is done, which isn't one more time.
  run(d, 127, [{ op: "task.update", id: id(1), due: { date: "2026-11-09", repeat: weekly } }]);
  run(d, 128, [{ op: "task.update", id: id(1), done: true, today: "2026-11-11" }]);
  run(d, 129, [{ op: "task.update", id: id(1), due: { date: "2026-11-16" }, done: true }]);
  expect(d.tasks.get(id(1))).toMatchObject({ due: { date: "2026-11-16" }, completions: { count: 1, last: "2026-11-11" } });
  expect(d.tasks.get(id(1))!.done_at).not.toBeNull();
  const doneRepeating: Op = { op: "task.create", id: id(2), title: "交房租", done: true, due: { date: "2026-11-30", repeat: weekly } };
  expect(() => apply(d, [doneRepeating], cx)).toThrow("“交房租”已完成，不能设置重复，请先重新打开");
});

test("a repeat by the lunar calendar is due within its table, an undo setting the date back too", () => {
  const d = empty();
  run(d, 150, [{ op: "task.create", id: id(1), title: "百年后", due: { date: "2101-02-01" } }]);
  run(d, 151, [{ op: "task.update", id: id(1), due: { date: "2101-01-28" } }]);
  const lunar: Repeat = { every: 12, unit: "month", from: "due", days: [29], lunar: true };
  run(d, 152, [{ op: "task.update", id: id(1), due: { date: "2101-01-28", repeat: lunar } }]);
  expect(() => apply(d, [{ op: "undo", mutation: id(151) }], cx)).toThrow("“百年后”的按农历重复的截止日期应在 1900-01-31 到 2101-01-28 之间");
});

test("a repeat until a day is done for good the last time, which counts, and undone repeats again", () => {
  const d = empty();
  const until = { every: 1, unit: "day", from: "due", until: "2026-12-19" } as const;
  run(d, 140, [{ op: "task.create", id: id(1), title: "练一套卷子", start: "2026-12-18", due: { date: "2026-12-18", repeat: until } }]);
  expect(() => apply(d, [{ op: "task.update", id: id(1), due: { date: "2026-12-20", repeat: until } }], cx)).toThrow("“练一套卷子”的重复到不能早于截止日期");
  // Nor done at once, which would take the repeat away with it.
  const late: Op = { op: "task.update", id: id(1), due: { date: "2026-12-20", repeat: until }, done: true, today: "2026-12-18" };
  expect(() => apply(d, [late], cx)).toThrow("“练一套卷子”的重复到不能早于截止日期");
  run(d, 141, [{ op: "task.update", id: id(1), done: true, today: "2026-12-18" }]);
  expect(d.tasks.get(id(1))).toMatchObject({ done_at: null, start: "2026-12-19", due: { date: "2026-12-19", repeat: until } });
  run(d, 142, [{ op: "task.update", id: id(1), done: true, today: "2026-12-19" }]);
  expect(d.tasks.get(id(1))).toMatchObject({ start: "2026-12-19", due: { date: "2026-12-19" }, completions: { count: 2, last: "2026-12-19" } });
  expect(d.tasks.get(id(1))!.done_at).not.toBeNull();
  expect(d.tasks.get(id(1))!.due!.repeat).toBeUndefined();
  run(d, 143, [{ op: "undo", mutation: id(142) }]);
  expect(d.tasks.get(id(1))).toMatchObject({ done_at: null, due: { date: "2026-12-19", repeat: until }, completions: { count: 1, last: "2026-12-18" } });
});

test("a start keeps its time with a new date, loses it with the date, and is not after the due time that day", () => {
  const d = empty();
  const daily = { every: 1, unit: "day", from: "due" } as const;
  run(d, 130, [{ op: "task.create", id: id(1), title: "抢票", start: "2026-11-12", start_time: "20:00", due: { date: "2026-11-13", repeat: daily } }]);
  run(d, 131, [{ op: "task.update", id: id(1), start: "2026-11-13" }]);
  expect(d.tasks.get(id(1))).toMatchObject({ start: "2026-11-13", start_time: "20:00" });
  // Done, it moves on a day, at the same time.
  run(d, 132, [{ op: "task.update", id: id(1), done: true, today: "2026-11-13" }]);
  expect(d.tasks.get(id(1))).toMatchObject({ start: "2026-11-14", start_time: "20:00", due: { date: "2026-11-14" } });
  expect(() => apply(d, [{ op: "task.update", id: id(1), due: { date: "2026-11-14", time: "19:00" } }], cx)).toThrow("“抢票”的开始时间不能晚于截止时间");
  expect(() => apply(d, [{ op: "task.update", id: id(1), start: null, start_time: "08:00" }], cx)).toThrow("设置开始的几点需要先有开始日期");
  // Cleared, the time goes with the date, and comes back with it.
  run(d, 133, [{ op: "task.update", id: id(1), start: null }]);
  expect(d.tasks.get(id(1))!.start_time).toBeUndefined();
  run(d, 134, [{ op: "undo", mutation: id(133) }]);
  expect(d.tasks.get(id(1))).toMatchObject({ start: "2026-11-14", start_time: "20:00" });
  // Undoing a new time once the date is gone brings back no time without a date.
  run(d, 135, [{ op: "task.update", id: id(1), start_time: "09:00" }]);
  run(d, 136, [{ op: "task.update", id: id(1), start: null }]);
  run(d, 137, [{ op: "undo", mutation: id(135) }]);
  expect([d.tasks.get(id(1))!.start, d.tasks.get(id(1))!.start_time]).toEqual([null, undefined]);
});

test("revert undoes what replaying effects did", () => {
  const d = empty();
  const effects = run(d, 110, [
    { op: "task.create", id: id(1), title: "一" },
    { op: "task.update", id: id(1), title: "二" },
  ]);
  revert(d, effects);
  expect(d.tasks.size).toBe(0);
});

test("AI jobs: only a failed one is retried, they go with their folder, and one being organized comes back failed", () => {
  const d = empty();
  const job = (state: "waiting" | "running" | "failed") => ({ ...d.jobs.get(id(2))!, state, error: state === "failed" ? "接口超时" : null });
  run(d, 110, [
    { op: "folder.create", id: id(1), name: "读书" },
    { op: "ai_job.create", id: id(2), text: "读完第三章", local_time: "2026-11-04 16:30 星期三", folder_id: id(1) },
  ]);
  expect(d.jobs.get(id(2))!.state).toBe("waiting");
  expect(() => apply(d, [{ op: "ai_job.retry", id: id(2) }], cx)).toThrow("这条请求已在等待整理");

  // As the server has it by now: organizing, then failed.
  d.jobs.set(id(2), job("running"));
  expect(() => apply(d, [{ op: "ai_job.retry", id: id(2), text: "改" }], cx)).toThrow("正在整理，稍后再改");
  d.jobs.set(id(2), job("failed"));
  run(d, 111, [{ op: "ai_job.retry", id: id(2), text: "读完第四章", folder_id: null }]);
  expect(d.jobs.get(id(2))).toMatchObject({ state: "waiting", error: null, text: "读完第四章", folder_id: null });

  // Back in the folder and organizing: deleting the folder takes it, undoing brings it back failed.
  d.jobs.set(id(2), { ...job("running"), folder_id: id(1) });
  run(d, 113, [{ op: "folder.delete", id: id(1) }]);
  expect(d.jobs.size).toBe(0);
  run(d, 114, [{ op: "undo", mutation: id(113) }]);
  expect(d.jobs.get(id(2))).toMatchObject({ state: "failed", error: "已撤销", folder_id: id(1) });
});

test("AI jobs: a tidying with suggestions is tidied again from scratch, and comes back as it was once taken and undone", () => {
  const d = empty();
  run(d, 120, [
    { op: "task.create", id: id(1), title: "官网" },
    { op: "ai_job.create", id: id(2), kind: "tidy", text: "整理一下", local_time: "2026-11-07 10:00 星期六" },
  ]);
  expect(d.jobs.get(id(2))).toMatchObject({ kind: "tidy", state: "waiting", suggestions: null });
  // As the server has it by now: ready, with what AI suggests.
  const ready = { ...d.jobs.get(id(2))!, state: "ready" as const, suggestions: [{ task: id(1), title: "建好官网", after: [], reason: "以动作开头", was: { title: "官网", description: "", due: null } }] };
  d.jobs.set(id(2), ready);
  run(d, 121, [
    { op: "task.update", id: id(1), title: "建好官网" },
    { op: "ai_job.delete", id: id(2) },
  ]);
  expect(d.jobs.size).toBe(0);
  run(d, 122, [{ op: "undo", mutation: id(121) }]);
  expect(d.tasks.get(id(1))!.title).toBe("官网");
  expect(d.jobs.get(id(2))).toMatchObject({ state: "ready", suggestions: ready.suggestions });
  run(d, 123, [{ op: "ai_job.retry", id: id(2) }]);
  expect(d.jobs.get(id(2))).toMatchObject({ state: "waiting", suggestions: null });
});

const [ALICE, BOB, CAROL] = ["0000000000aa", "0000000000bb", "0000000000cc"].map((n) => `019a4b2c-0000-7000-8000-${n}`);

/** The user's folder 装修 (1), shared with Bob, with 量尺寸 (3) and 买瓷砖 (4) in it; their private
 * folder 私人 (2) with 日记 (5); and Alice's folder 新家 (6), shared with the user, with 选灯 (7). */
function sharing(): Data {
  const d = empty();
  run(d, 200, [
    { op: "folder.create", id: id(1), name: "装修" },
    { op: "folder.create", id: id(2), name: "私人" },
    { op: "task.create", id: id(3), title: "量尺寸", folder_id: id(1) },
    { op: "task.create", id: id(4), title: "买瓷砖", folder_id: id(1) },
    { op: "task.create", id: id(5), title: "日记", folder_id: id(2) },
    { op: "folder.create", id: id(6), name: "新家" },
    { op: "task.create", id: id(7), title: "选灯", folder_id: id(6) },
  ]);
  d.shares.set(id(1), { folder_id: id(1), owner: ME, members: [BOB] });
  d.shares.set(id(6), { folder_id: id(6), owner: ALICE, members: [ME] });
  for (const t of d.tasks.values()) d.tasks.set(t.id, { ...t, number: 5 });
  return d;
}

test("sharing: tasks link only within one place, and only those a shared folder is shared with are assigned", () => {
  const d = sharing();
  run(d, 201, [{ op: "dep.add", before: id(3), after: id(4) }]);
  const apart = "只能连同一处的事项：私人的事项之间，或同一个共享的文件夹里的事项之间";
  expect(() => apply(d, [{ op: "dep.add", before: id(3), after: id(5) }], cx)).toThrow(apart);
  expect(() => apply(d, [{ op: "dep.add", before: id(7), after: id(4) }], cx)).toThrow(apart);
  expect(() => apply(d, [{ op: "task.update", id: id(5), assign: [ME] }], cx)).toThrow("只有共享的文件夹里的事项能设负责人");
  expect(() => apply(d, [{ op: "task.update", id: id(3), assign: [CAROL] }], cx)).toThrow("负责人须是这个文件夹的共享人或成员");
  run(d, 202, [{ op: "task.update", id: id(3), assign: [BOB, ME] }]);
  expect(d.tasks.get(id(3))!.assignees).toEqual([BOB, ME]);
});

test("sharing: moving in or out cuts the links and the assignees that don't fit, and leaves the number to the server", () => {
  const d = sharing();
  run(d, 210, [
    { op: "dep.add", before: id(3), after: id(4) },
    { op: "task.update", id: id(3), assign: [BOB] },
  ]);
  // Out to the user's own, still theirs: the number stays.
  const out = run(d, 211, [{ op: "task.update", id: id(3), folder_id: id(2) }]);
  expect(out.map((e) => e.kind)).toEqual(["dependency", "task"]);
  expect(d.tasks.get(id(3))).toMatchObject({ assignees: [], number: 5 });
  run(d, 212, [{ op: "undo", mutation: id(211) }]);
  expect(d.tasks.get(id(3))).toMatchObject({ folder_id: id(1), assignees: [BOB], number: 5 });
  expect(d.deps.size).toBe(1);
  // Into Alice's: hers, numbered by the server; and back, the user's again, numbered by it too.
  run(d, 213, [{ op: "task.update", id: id(5), folder_id: id(6) }]);
  expect(d.tasks.get(id(5))!.number).toBe(0);
  run(d, 214, [{ op: "undo", mutation: id(213) }]);
  expect(d.tasks.get(id(5))).toMatchObject({ folder_id: id(2), number: 0 });
});

test("sharing: undoing a move links nothing again that has been moved to another place since", () => {
  const d = sharing();
  run(d, 230, [
    { op: "task.create", id: id(8), title: "设计" },
    { op: "task.create", id: id(9), title: "开发" },
    { op: "dep.add", before: id(8), after: id(9) },
  ]);
  run(d, 231, [{ op: "task.update", id: id(9), folder_id: id(1) }]);
  run(d, 232, [{ op: "task.update", id: id(8), folder_id: id(6) }]);
  run(d, 233, [{ op: "undo", mutation: id(231) }]);
  expect([d.tasks.get(id(9))!.folder_id, d.deps.size]).toEqual([null, 0]);
});

test("sharing: a shared folder is its owner's to change, goes into no other, and people are added online", () => {
  const d = sharing();
  const owners = "只有共享人能改名、移动或删除共享的文件夹";
  expect(() => apply(d, [{ op: "folder.update", id: id(6), name: "旧家" }], cx)).toThrow(owners);
  expect(() => apply(d, [{ op: "folder.delete", id: id(6) }], cx)).toThrow(owners);
  expect(() => apply(d, [{ op: "folder.update", id: id(1), parent_id: id(6) }], cx)).toThrow("共享的文件夹不能放进另一个共享的文件夹");
  run(d, 220, [{ op: "folder.update", id: id(1), parent_id: id(2) }]);
  expect(() => apply(d, [{ op: "folder.update", id: id(2), parent_id: id(6) }], cx)).toThrow("共享的文件夹不能放进另一个共享的文件夹");
  expect(() => apply(d, [{ op: "share.add", folder_id: id(1), username: "carol" }], cx)).toThrow("添加、移除成员需要联网");
});
