import { expect, test } from "vitest";
import cases from "../../../../fixtures/repeat.json";
import type { Repeat } from "../api/types/Repeat";
import { nextDue } from "./repeat";

test.each(cases)("$name", (c) => {
  expect(nextDue(c.repeat as Repeat, c.due, c.today)).toBe(c.next);
});
