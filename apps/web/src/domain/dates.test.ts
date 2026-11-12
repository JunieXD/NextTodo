import { expect, test } from "vitest";
import { deadline, dueKind, formatDue, parseTime, remindLabel } from "./dates";

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

test("reminders", () => {
  expect([undefined, 0, 30, 120, 2880].map(remindLabel)).toEqual(["不提醒", "截止时", "提前 30 分钟", "提前 2 小时", "提前 2 天"]);
  // Not a whole number of a larger unit, as an agent may set it.
  expect([90, 1500].map(remindLabel)).toEqual(["提前 90 分钟", "提前 25 小时"]);
});

test("times typed", () => {
  expect(["9:07", "09:07", "907", "0907", "9：07", "9.07", " 21:30 ", "9", "0", "23:59"].map(parseTime)).toEqual([
    "09:07", "09:07", "09:07", "09:07", "09:07", "09:07", "21:30", "09:00", "00:00", "23:59",
  ]);
  expect(["", "24:00", "9:60", "9:7", "12345", "9点", "a"].map(parseTime)).toEqual([null, null, null, null, null, null, null]);
});
