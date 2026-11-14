// Links out of the app: into the website's guide (apps/site), which explains at length what the app
// says briefly, and to the QQ group. Opened outside the app: the desktop app and Android hand such
// links to the browser.

import type { ReactNode } from "react";

const GUIDE = "https://juniexd.github.io/NextTodo/guide/";

/** Where users ask and report problems, as the README has it; the link opens QQ to join it. */
export const QQ_GROUP = { number: "1135479637", url: "https://qm.qq.com/q/yz6W1Q9NFS" };

/** A link to the page `to` of the guide, such as `ai` or `#modes` (the guide's first page). */
export function Help({ to, children }: { to: string; children: ReactNode }) {
  return (
    <a className="help" href={GUIDE + to} target="_blank" rel="noreferrer">
      {children}
    </a>
  );
}
