import { expect, test } from "vitest";
import { newer } from "./server-update";

test("versions compare by their numbers", () => {
  expect(newer("0.1.150", "0.1.145")).toBe(true);
  expect(newer("0.1.145", "0.1.145")).toBe(false);
  expect(newer("0.1.99", "0.1.100")).toBe(false);
  expect(newer("0.2.0", "0.1.999")).toBe(true);
});
