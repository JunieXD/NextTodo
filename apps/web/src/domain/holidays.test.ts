import { expect, test } from "vitest";
import cases from "../../../../fixtures/holidays.json";
import { festival, holiday, solarTerm } from "./holidays";

test.each(cases)("$date", (c) => {
  expect([festival(c.date), solarTerm(c.date), holiday(c.date)]).toEqual([c.festival, c.term, c.holiday]);
});
