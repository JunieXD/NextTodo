import { expect, onTestFinished, test, vi } from "vitest";
import { deadline, dueIn, dueKind, formatDue, inOrder, nextFive, parseTime, remindAt, remindersLabel } from "./dates";

// Monday 2 November 2026, 10:30
const now = new Date(2026, 10, 2, 10, 30);

test("due dates read relative to today", () => {
  expect(formatDue({ date: "2026-11-02", time: "18:00" }, now)).toBe("今天 18:00");
  expect(formatDue({ date: "2026-11-01" }, now)).toBe("昨天");
  expect(formatDue({ date: "2026-11-03" }, now)).toBe("明天");
  expect(formatDue({ date: "2026-11-06" }, now)).toBe("周五");
  expect(formatDue({ date: "2026-11-09", time: "09:00" }, now)).toBe("11月9日 09:00");
  expect(formatDue({ date: "2027-03-15" }, now)).toBe("2027年3月15日");
});

test("a date alone is due at the end of that day", () => {
  expect(deadline({ date: "2026-11-02" })).toEqual(new Date(2026, 10, 3));
  expect(dueKind({ date: "2026-11-02" }, now)).toBe("today");
  expect(dueKind({ date: "2026-11-02", time: "09:00" }, now)).toBe("overdue");
  expect(dueKind({ date: "2026-11-09" }, now)).toBe("week");
  expect(dueKind({ date: "2026-11-10" }, now)).toBe("later");
});

test("how long until it is due, or since", () => {
  // By the hour within a day of a due time, by the minute within an hour.
  expect(dueIn({ date: "2026-11-02", time: "18:00" }, now)).toBe("还有 7 小时");
  expect(dueIn({ date: "2026-11-03", time: "09:00" }, now)).toBe("还有 22 小时");
  expect(dueIn({ date: "2026-11-02", time: "10:50" }, now)).toBe("还有 20 分钟");
  expect(dueIn({ date: "2026-11-02", time: "10:30" }, now)).toBe("逾期不到 1 分钟");
  expect(dueIn({ date: "2026-11-02", time: "10:31" }, new Date(2026, 10, 2, 10, 30, 20))).toBe("还有不到 1 分钟");
  expect(dueIn({ date: "2026-11-01", time: "20:00" }, now)).toBe("逾期 14 小时");
  // Otherwise by the date.
  expect(dueIn({ date: "2026-11-03", time: "11:00" }, now)).toBe("还有 1 天");
  expect(dueIn({ date: "2026-11-03" }, now)).toBe("还有 1 天");
  expect(dueIn({ date: "2026-10-31", time: "18:00" }, now)).toBe("逾期 2 天");
  expect(dueIn({ date: "2026-11-01" }, now)).toBe("逾期 1 天");
  // A date alone due today: `今天` says it.
  expect(dueIn({ date: "2026-11-02" }, now)).toBeNull();
});

test("a due time on a day of 25 hours is still by the hour", () => {
  vi.stubEnv("TZ", "America/New_York");
  onTestFinished(() => void vi.unstubAllEnvs());
  // 1 November 2026: the clocks go back an hour at 2:00.
  expect(dueIn({ date: "2026-11-01", time: "23:30" }, new Date(2026, 10, 1, 0, 0))).toBe("还有 24 小时");
});

test("reminders", () => {
  expect(remindersLabel(undefined)).toBe("不提醒");
  expect(remindersLabel([2880, 120].map((before) => ({ before })))).toBe("提前 2 天、提前 2 小时");
  // Not a whole number of a larger unit, as an agent may set it.
  expect(remindersLabel([{ before: 90 }, { before: 1500 }])).toBe("提前 90 分钟、提前 25 小时");
  // More than two, the first and how many, kept to a row.
  expect(remindersLabel([3, 1, 0].map((days) => ({ days, time: "09:00" })))).toBe("提前 3 天 09:00 等 3 个");
  // As the server keeps them: the first to come first, each once, none before a time there isn't.
  const all = [{ before: 0 }, { days: 0, time: "08:00" }, { before: 1440 }, { days: 1, time: "20:00" }, { before: 0 }];
  expect(inOrder(all, "09:00")).toEqual([{ before: 1440 }, { days: 1, time: "20:00" }, { days: 0, time: "08:00" }, { before: 0 }]);
  expect(inOrder(all, undefined)).toEqual([{ days: 1, time: "20:00" }, { days: 0, time: "08:00" }]);
  // By the day, on the date or before it at the time; before the time, by the minute.
  const due = { date: "2026-11-08", time: "09:00" };
  expect(remindAt(due, { days: 1, time: "20:00" })).toEqual(new Date(2026, 10, 7, 20, 0));
  expect(remindAt(due, { before: 90 })).toEqual(new Date(2026, 10, 8, 7, 30));
  expect(remindAt({ date: "2026-12-01" }, { days: 2, time: "09:30" })).toEqual(new Date(2026, 10, 29, 9, 30));
});

test("the next five minutes", () => {
  const at = (h: number, m: number, s: number) => nextFive(new Date(2026, 10, 2, h, m, s));
  expect([at(3, 42, 20), at(3, 45, 0), at(3, 45, 30), at(3, 49, 59), at(23, 54, 0), at(23, 55, 0), at(23, 59, 59)]).toEqual([
    "03:45", "03:50", "03:50", "03:50", "23:55", "23:55", "23:55",
  ]);
});

test("times typed", () => {
  expect(["9:07", "09:07", "907", "0907", "9：07", "9.07", " 21:30 ", "9", "0", "23:59"].map(parseTime)).toEqual([
    "09:07", "09:07", "09:07", "09:07", "09:07", "09:07", "21:30", "09:00", "00:00", "23:59",
  ]);
  expect(["", "24:00", "9:60", "9:7", "12345", "9点", "a"].map(parseTime)).toEqual([null, null, null, null, null, null, null]);
});
