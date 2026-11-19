import { expect, test } from "vitest";
import { type Data, apply, fromSnapshot, toSnapshot } from "./apply";
import { Board } from "./board";
import { copyFolder, copyName } from "./copy";

const ME = "019a4b2c-0000-7000-8000-0000000000ee";
const id = (n: number) => `019a4b2c-0000-7000-8000-${String(n).padStart(12, "0")}`;
const cx = { me: ME, now: 1000, effectsOf: () => undefined, effectsAfter: () => [] };
const board = (d: Data) => new Board(toSnapshot(d), { today: "2026-11-20" }, ME);

test("a copy is named after the folder, or as a copy of it when that is taken", () => {
  const taken = (names: string[]) => (n: string) => names.includes(n);
  expect(copyName("合同", taken([]))).toBe("合同");
  expect(copyName("合同", taken(["合同"]))).toBe("合同 副本");
  expect(copyName("合同", taken(["合同", "合同 副本", "合同 副本 2"]))).toBe("合同 副本 3");
});

test("a folder copies with all in it, open again, and its attachments referred to anew", () => {
  const d = fromSnapshot({ folders: [], tasks: [], dependencies: [], attachments: [], ai_jobs: [], shares: [], people: [] });
  apply(
    d,
    [
      { op: "folder.create", id: id(1), name: "合同" },
      { op: "folder.create", id: id(2), name: "盖章", parent_id: id(1) },
      { op: "task.create", id: id(9), title: "外部审批" },
      { op: "task.create", id: id(3), title: "审价", folder_id: id(1), description: `见[报价](attachment:${id(5)})`, done: true },
      { op: "task.create", id: id(4), title: "盖章", folder_id: id(2), due: { date: "2026-11-20", time: "18:00", repeat: { every: 1, unit: "week", from: "due" } } },
      { op: "dep.add", before: id(9), after: id(3) },
      { op: "dep.add", before: id(3), after: id(4) },
      { op: "attachment.add", id: id(5), name: "报价.txt", type: "text/plain", size: 5, task_id: id(3) },
    ],
    cx,
  );
  const { ops, files } = copyFolder(board(d), id(1), null, { id: id(20), name: "合同 副本", sort_key: "a5" });
  apply(d, ops, cx);
  const b = board(d);
  const copy = b.folder(id(20))!;
  expect(b.subfolders(copy.id).map((f) => f.name)).toEqual(["盖章"]);
  const [review] = b.contents(copy.id);
  expect([review.title, review.done_at, b.status(review)]).toEqual(["审价", null, "ready"]);
  const [attachment] = b.attachments(review.id);
  expect(files).toEqual([[attachment.id, b.attachments(id(3))[0]]]);
  expect(review.description).toBe(`见[报价](attachment:${attachment.id})`);
  const [stamp] = b.contents(b.subfolders(copy.id)[0].id);
  expect(stamp.due).toEqual(b.task(id(4))!.due);
  expect(b.before(stamp).map((t) => t.id)).toEqual([review.id]);
});

test("tasks keep who they are assigned to only in the shared folder they were in", () => {
  const BOB = "019a4b2c-0000-7000-8000-0000000000bb";
  const d = fromSnapshot({
    folders: [
      { id: id(1), parent_id: null, name: "项目", sort_key: "a0", created_at: 0, updated_at: 0 },
      { id: id(2), parent_id: id(1), name: "采购", sort_key: "a0", created_at: 0, updated_at: 0 },
    ],
    tasks: [],
    dependencies: [],
    attachments: [],
    ai_jobs: [],
    shares: [{ folder_id: id(2), owner: ME, members: [BOB] }],
    people: [{ id: BOB, username: "bob", display_name: "Bob" }],
  });
  apply(d, [{ op: "task.create", id: id(3), title: "比价", folder_id: id(2), assignees: [BOB] }], cx);
  // The private folder holding it copies to a private one.
  apply(d, copyFolder(board(d), id(1), null, { id: id(10), name: "项目 副本", sort_key: "a1" }).ops, cx);
  const b = board(d);
  const [buying] = b.subfolders(id(10));
  expect(b.share(buying.id)).toBeUndefined();
  expect(b.contents(buying.id).map((t) => t.assignees)).toEqual([[]]);
  // Within the shared folder, it stays theirs to do.
  apply(d, copyFolder(b, id(2), id(2), { id: id(11), name: "采购 副本", sort_key: "a1" }).ops, cx);
  expect(board(d).contents(id(11)).map((t) => t.assignees)).toEqual([[BOB]]);
});
