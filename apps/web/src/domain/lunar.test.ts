import { expect, test } from "vitest";
import cases from "../../../../fixtures/lunar.json";
import { lunarDate, lunarText } from "./lunar";

test.each(cases)("$date", (c) => {
  const l = lunarDate(c.date);
  expect(l && lunarText(l)).toBe(c.lunar ?? undefined);
});
