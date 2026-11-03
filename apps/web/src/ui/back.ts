// What Android's back button does: close what is on top (a dialog or menu, then the task's details
// or linking in the graph) or step out of a folder; with nothing left, leave the app. Pages side by
// side, such as the tabs, are not steps back.

import { useEffect, useRef } from "react";

/** Where a step is, from the bottom: the page, a panel over it, a dialog or menu over everything.
 * Back takes the top layer's latest step, whenever each was mounted. */
const LAYERS = ["page", "panel", "dialog"] as const;
type Layer = (typeof LAYERS)[number];

const steps: { layer: number; take: () => void }[] = [];

/** While `step` is given, back calls it before the steps below it. */
export function useBack(layer: Layer, step: (() => void) | null) {
  const latest = useRef(step);
  useEffect(() => {
    latest.current = step;
  });
  const on = step !== null;
  useEffect(() => {
    if (!on) return;
    const entry = { layer: LAYERS.indexOf(layer), take: () => latest.current?.() };
    steps.push(entry);
    return () => void steps.splice(steps.indexOf(entry), 1);
  }, [on, layer]);
}

/** Takes the step on top; false when there is none. */
export function back(): boolean {
  const top = steps.reduce<(typeof steps)[number] | undefined>((top, s) => (!top || s.layer >= top.layer ? s : top), undefined);
  top?.take();
  return top !== undefined;
}
