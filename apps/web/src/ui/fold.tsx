// What unfolds into place and folds away: the completed tasks at the bottom of a folder, the folders
// in the tree (docs/ui.md#交互细节).

import { type ReactNode, useState } from "react";

type State = "open" | "opening" | "closing" | "closed";

/** `children` while `open`: unfolding as it opens, folding away before they go. Open when it comes,
 * with the page say, they are just there. */
export function Fold({ open, children }: { open: boolean; children: ReactNode }) {
  const [state, setState] = useState<State>(open ? "open" : "closed");
  if (open !== (state === "open" || state === "opening")) setState(open ? "opening" : "closing");
  if (state === "closed") return null;
  return (
    <div className={`fold ${state}`} onAnimationEnd={(e) => e.target === e.currentTarget && setState(open ? "open" : "closed")}>
      <div>{children}</div>
    </div>
  );
}
