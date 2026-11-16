// Which releases a device is told of after an update, which goes by what the device had stored when
// the app opened, and how the changes of several are listed together.

import { beforeEach, expect, test, vi } from "vitest";
import type { Release } from "./changelog";

beforeEach(() => {
  localStorage.clear();
  // A release build. Taken away rather than set to false: in tests, env values are strings.
  vi.stubEnv("WEB_DEV", undefined);
  // Later than any release, so that all of them are out by now.
  vi.stubEnv("WEB_VERSION", "1.0.0");
});

/** The app as it opens with `stored` on the device. */
async function open(stored: Record<string, string>) {
  for (const [key, value] of Object.entries(stored)) localStorage.setItem(key, value);
  vi.resetModules();
  return import("./changelog");
}

test("a device updated from a version that noted none is told of those since 0.1.201", async () => {
  const { RELEASES, unseen } = await open({ "nexttodo.count": "1" });
  const news = unseen()!;
  expect(news.since).toBe("0.1.201");
  expect(news.releases[0].version).toBe(RELEASES[0].version);
  expect(news.releases.map((r) => r.version)).not.toContain("0.1.201");
});

test("a device opening the app for the first time is told of none, signed in to meanwhile or not", async () => {
  const { unseen } = await open({});
  expect(unseen()).toBeNull();
  localStorage.setItem("nexttodo.count", "1");
  expect(unseen()).toBeNull();
});

test("the changes of several releases are listed by kind, new features first, the latest release first in each", async () => {
  const { byKind } = await open({});
  const release = (version: string, changes: [string, string][]): Release => ({
    version,
    date: "",
    changes: changes.map(([kind, text]) => ({ kind, text, version })),
  });
  const listed = byKind([
    release("0.1.3", [["修复", "c"]]),
    release("0.1.2", [["新功能", "a"], ["改进", "b"], ["修复", "d"]]),
  ]);
  expect(listed.map(({ kind, changes }) => [kind, changes.map((c) => `${c.text} ${c.version}`)])).toEqual([
    ["新功能", ["a 0.1.2"]],
    ["改进", ["b 0.1.2"]],
    ["修复", ["c 0.1.3", "d 0.1.2"]],
  ]);
});
