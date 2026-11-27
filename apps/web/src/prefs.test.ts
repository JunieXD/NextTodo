import { beforeEach, expect, test } from "vitest";
import type { Prefs } from "./api/types/Prefs";
import type { User } from "./api/types/User";
import { followPrefs, prefsOf } from "./prefs";
import { Store } from "./sync/store";

const ME = "019a4b2c-0000-7000-8000-0000000000ee";
const user: User = { id: ME, username: "me", display_name: "我", is_admin: false };

/** A window following the settings, its copy having the account's or, `undefined`, not knowing them. */
function following(prefs: Prefs | undefined) {
  const { prefs: _, ...base } = Store.empty().base;
  const store = new Store(ME, { ...Store.empty(), base: { ...base, ...(prefs && { prefs }) } }, {
    save: () => Promise.resolve(true),
    send() {},
    uploaded: () => true,
    awaiting() {},
    rejected() {},
    arrived() {},
    confirmed() {},
  });
  const stop = followPrefs({ store, user });
  const made = () => store.queued.flatMap((m) => m.ops);
  const known = (prefs: Prefs) => store.resync({ ...base, seq: 1, user: ME, prefs });
  return { store, made, known, stop };
}

beforeEach(() => localStorage.clear());

test("this device's own settings go into the account once it is known, where it has none", async () => {
  localStorage.setItem("nexttodo.accent", "blue");
  localStorage.setItem("nexttodo.keepDone", "5");
  localStorage.setItem("nexttodo.timeGrid", "off");
  localStorage.setItem(`nexttodo.addToFixed:${ME}`, "null");
  // A copy from before the update, or from a server too old to keep them: this device's are in effect.
  const first = following(undefined);
  expect(first.made()).toEqual([]);
  expect(prefsOf(first.store.getView().board)).toEqual({ accent: "blue", time_grid: false, keep_done: 5, add_to: { folder: null } });

  first.known({ keep_done: 3 });
  expect(first.made()).toEqual([{ op: "prefs.set", accent: "blue", time_grid: false, keep_done: 5, add_to: { folder: null }, if_unset: true }]);
  expect(prefsOf(first.store.getView().board)).toEqual({ accent: "blue", time_grid: false, keep_done: 3, add_to: { folder: null } });
  await first.store.saved();
  first.stop();

  // Then the account's, whosever it is: another signing in on this device has theirs.
  const second = following({ accent: "#336699" });
  expect(second.made()).toEqual([]);
  expect(localStorage.getItem("nexttodo.accent")).toBe("#336699");
  expect(prefsOf(second.store.getView().board)).toEqual({ accent: "#336699" });
});

test("the accent kept on this device follows the account's; a colour being dragged meanwhile stays", () => {
  localStorage.setItem("nexttodo.prefsMoved", "1");
  const { store } = following({});
  store.commit([{ op: "prefs.set", accent: "pink" }]);
  expect(localStorage.getItem("nexttodo.accent")).toBe("pink");

  localStorage.setItem("nexttodo.accent", "#123456");
  store.commit([{ op: "prefs.set", holidays: false }]);
  expect(localStorage.getItem("nexttodo.accent")).toBe("#123456");
  store.commit([{ op: "prefs.set", accent: null }]);
  expect(localStorage.getItem("nexttodo.accent")).toBeNull();
});
