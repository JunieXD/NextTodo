// The settings that go with the account (docs/ui/settings.md#跟着账号的设置), which the user's data holds.
// Each device kept its own before; it still has them, for until the account's are known: in a copy from
// before the update, or from a server too old to keep them (docs/design/updates.md).

import type { Prefs } from "./api/types/Prefs";
import type { Board } from "./domain/board";
import { keep } from "./kept";
import type { Session } from "./session";

/** Where this device kept them. The accent stays there too, for the theme to show before a sign-in. */
export const ACCENT = "nexttodo.accent";
const DEFAULT_REMINDERS = "nexttodo.defaultReminders";
const TIME_GRID = "nexttodo.timeGrid";
const HOLIDAY_MARKS = "nexttodo.holidayMarks";
const KEEP_DONE = "nexttodo.keepDone";
const fixedAddTo = (user: string) => `nexttodo.addToFixed:${user}`;

/** Set once this device's own have gone into an account: from then on it has the account's. */
const MOVED = "nexttodo.prefsMoved";

/** This device's own, those it has. */
function devicePrefs(user: string): Prefs {
  const read = (key: string) => {
    const kept = localStorage.getItem(key);
    return kept === null ? undefined : JSON.parse(kept);
  };
  const on = (key: string) => {
    const kept = localStorage.getItem(key);
    return kept === null ? undefined : kept !== "off";
  };
  const fixed = localStorage.getItem(fixedAddTo(user));
  return {
    reminders: read(DEFAULT_REMINDERS),
    accent: localStorage.getItem(ACCENT) ?? undefined,
    time_grid: on(TIME_GRID),
    holidays: on(HOLIDAY_MARKS),
    keep_done: read(KEEP_DONE),
    add_to: fixed === null ? undefined : { folder: JSON.parse(fixed) },
  };
}

/** The settings in effect: the account's, or else, until they are known, this device's. */
export const prefsOf = (board: Board): Prefs => board.snapshot.prefs ?? devicePrefs(board.me);

type Following = Pick<Session, "store" | "user">;

/** Once the account's settings are known, has this device's own go into it where it has none yet, and
 * the accent kept on this device follow the account's; returns how to stop. */
export function followPrefs({ store, user }: Following): () => void {
  let moving = false;
  let accent: string | null | undefined;
  const follow = () => {
    if (!store.getView().board.snapshot.prefs) return;
    if (!moving && localStorage.getItem(MOVED) === null) {
      moving = true;
      const mine = devicePrefs(user.id);
      // Only into what is not set when the server makes it, which may know more than this copy: another
      // device's may have gone in first. Tried again on the next change, should saving it here fail.
      if (Object.values(mine).some((v) => v !== undefined)) store.commit([{ op: "prefs.set", ...mine, if_unset: true }]);
      void store.saved().then((saved) => (saved ? localStorage.setItem(MOVED, "1") : (moving = false)));
    }
    // Only as the account's changes: a colour of the user's own being dragged meanwhile stays.
    const now = store.getView().board.snapshot.prefs?.accent ?? null;
    if (now !== accent) keep(ACCENT, (accent = now));
  };
  follow();
  return store.subscribe(follow);
}
