// Which releases a device is told of after an update: it goes by what the device had stored when the
// app opened.

import { beforeEach, expect, test, vi } from "vitest";

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

const versions = (releases: { version: string }[]) => releases.map((r) => r.version);

test("a device updated from a version that noted none is told of those since 0.1.201", async () => {
  const { RELEASES, unseen } = await open({ "nexttodo.count": "1" });
  expect(versions(unseen())).toContain(RELEASES[0].version);
  expect(versions(unseen())).not.toContain("0.1.201");
});

test("a device opening the app for the first time is told of none, signed in to meanwhile or not", async () => {
  const { unseen } = await open({});
  expect(unseen()).toEqual([]);
  localStorage.setItem("nexttodo.count", "1");
  expect(unseen()).toEqual([]);
});
