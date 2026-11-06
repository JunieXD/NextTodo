// Overlays leave as they came, though React takes them out at once: their elements are put back for
// the moment their "leave" class plays (docs/ui.md#交互细节).

import { type RefObject, useLayoutEffect } from "react";

/** Has the elements `refs` hold, an overlay's, stay a moment after it unmounts, playing their "leave"
 * class, unless it has been `replaced` by then. */
export function useLeave(refs: RefObject<HTMLElement | null>[], replaced = () => false) {
  useLayoutEffect(() => {
    const els = refs.map((r) => r.current!);
    return () => {
      const parent = els[0].parentNode!;
      // An element's scroll goes with it out of the page.
      const scrolled = els.flatMap((el) => [...el.querySelectorAll("*")]).filter((e) => e.scrollTop).map((e) => [e, e.scrollTop] as const);
      // Once React has taken them out, which it doesn't in StrictMode's rehearsal of unmounting.
      queueMicrotask(() => {
        if (els[0].isConnected || replaced()) return;
        for (const el of els) {
          el.classList.remove("enter");
          el.classList.add("leave");
          el.inert = true;
          parent.append(el);
        }
        for (const [e, top] of scrolled) e.scrollTop = top;
        Promise.allSettled(els.flatMap((el) => el.getAnimations()).map((a) => a.finished)).then(() => els.forEach((el) => el.remove()));
      });
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the elements mounted, which stay
  }, []);
}
