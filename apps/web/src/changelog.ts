// What each release changed, from CHANGELOG.md, and which of those to tell of after an update
// (docs/design.md#更新日志).

import source from "../../../CHANGELOG.md?raw";
import { newer } from "./updates";
import { signedInBefore } from "./session";

/** A release: its version, the day it came out, and what it changed. */
export type Release = { version: string; date: string; changes: Change[] };

/** One item of a release's notes: its kind, the heading it is under, such as 新功能. */
export type Change = { kind: string; text: string; version: string };

/** All the releases, the latest first, as CHANGELOG.md has them: each a section headed
 * `## 0.1.212 · 2026-11-15`, its items under `### 新功能` and the like. */
export const RELEASES: Release[] = source
  .split(/^## /m)
  .slice(1)
  .map((section) => {
    const [head, ...lines] = section.split("\n");
    const [version, date] = head.split(" · ");
    const changes: Change[] = [];
    let kind = "";
    for (const line of lines) {
      if (line.startsWith("### ")) kind = line.slice(4).trim();
      else if (line.startsWith("- ")) changes.push({ kind, text: line.slice(2).trim(), version });
    }
    return { version, date, changes };
  });

/** The kinds in the order they are listed in; any other after them. */
const KINDS = ["新功能", "改进", "修复"];

const rank = (kind: string) => {
  const i = KINDS.indexOf(kind);
  return i < 0 ? KINDS.length : i;
};

/** The changes of `releases` together, by kind, each kind's in the order of `releases`. */
export function byKind(releases: Release[]): { kind: string; changes: Change[] }[] {
  const changes = releases.flatMap((r) => r.changes);
  const kinds = [...new Set(changes.map((c) => c.kind))].sort((a, b) => rank(a) - rank(b));
  return kinds.map((kind) => ({ kind, changes: changes.filter((c) => c.kind === kind) }));
}

/** The latest version this device has run, which it tells of updates since. */
const SEEN = "nexttodo.seenVersion";

const current = import.meta.env.WEB_VERSION;

/** The last release that did not note the version it ran as. */
const UNNOTED = "0.1.201";

/** The releases since the version this device last ran, up to this one, the latest first, and that
 * version: none the first time, nor for a development build. */
export function unseen(): { since: string; releases: Release[] } | null {
  // With none noted, a device signed in to before ran 0.1.201 or an earlier version.
  const since = localStorage.getItem(SEEN) ?? (signedInBefore ? UNNOTED : null);
  if (import.meta.env.WEB_DEV || !since) return null;
  const releases = RELEASES.filter((r) => newer(r.version, since) && !newer(r.version, current));
  return releases.length ? { since, releases } : null;
}

/** Notes this version as run, unless a later one was: an older one may run again, the Android app
 * going back from a build that would not start say, and its updates are told of already. */
export function seeVersion() {
  const seen = localStorage.getItem(SEEN);
  if (!import.meta.env.WEB_DEV && (!seen || newer(current, seen))) localStorage.setItem(SEEN, current);
}
