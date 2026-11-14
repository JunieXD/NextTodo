// What each release changed, from CHANGELOG.md, and which of those to tell of after an update
// (docs/design.md#更新日志).

import source from "../../../CHANGELOG.md?raw";
import { newer } from "./server-update";

/** A release: its version, the day it came out, and what it changed, in Markdown. */
export type Release = { version: string; date: string; notes: string };

/** All the releases, the latest first, as CHANGELOG.md has them: each a section headed
 * `## 0.1.212 · 2026-11-15`. */
export const RELEASES: Release[] = source
  .split(/^## /m)
  .slice(1)
  .map((section) => {
    const [head, ...notes] = section.split("\n");
    const [version, date] = head.split(" · ");
    return { version, date, notes: notes.join("\n").trim() };
  });

/** The latest version this device has run, which it tells of updates since. */
const SEEN = "nexttodo.seenVersion";

const current = import.meta.env.WEB_VERSION;

/** The releases since the version this device last ran, up to this one, the latest first: none the
 * first time, nor for a development build. */
export function unseen(): Release[] {
  const seen = localStorage.getItem(SEEN);
  if (import.meta.env.WEB_DEV || !seen) return [];
  return RELEASES.filter((r) => newer(r.version, seen) && !newer(r.version, current));
}

/** Notes this version as run, unless a later one was: an older one may run again, the Android app
 * going back from a build that would not start say, and its updates are told of already. */
export function seeVersion() {
  const seen = localStorage.getItem(SEEN);
  if (!import.meta.env.WEB_DEV && (!seen || newer(current, seen))) localStorage.setItem(SEEN, current);
}
