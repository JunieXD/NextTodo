// What Android's back button does: close what was opened last (a dialog, a panel, a menu, the task's
// details) or step out of a folder; with nothing left, leave the app. Pages side by side, such as
// the tabs, are not steps back.

import { useEffect, useRef } from "react";

const steps: (() => void)[] = [];

/** While `step` is given, back calls it, before the steps given earlier. */
export function useBack(step: (() => void) | null) {
  const latest = useRef(step);
  useEffect(() => {
    latest.current = step;
  });
  const on = step !== null;
  useEffect(() => {
    if (!on) return;
    const entry = () => latest.current?.();
    steps.push(entry);
    return () => void steps.splice(steps.indexOf(entry), 1);
  }, [on]);
}

/** Takes the latest step back; false when there is none. */
export function back(): boolean {
  const last = steps.at(-1);
  last?.();
  return last !== undefined;
}
