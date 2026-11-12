import { expect, test } from "vitest";
import cases from "../../../../fixtures/repeat.json";
import type { Repeat } from "../api/types/Repeat";
import { nextDue, shifted } from "./repeat";

test.each(cases)("$name", (c) => {
  expect(nextDue(c.repeat as Repeat, c.due, c.today)).toBe(c.next);
  if (c.start) expect(shifted(c.start, c.due, c.next!)).toBe(c.next_start);
});
