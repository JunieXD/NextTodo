// Where a folder is, folder by folder, each one a step there: above a folder's page, and in the task
// picker (docs/ui.md#布局).

import { Fragment } from "react";
import type { FolderId } from "../api/types/FolderId";
import type { Board } from "../domain/board";

/** `学习 / 毕业论文` from `from` down, `根目录` for the root; `go` takes each to its folder, but the last one
 * when it is `here`, where one is. */
export function Crumbs(p: { board: Board; folder: FolderId | null; from?: FolderId | null; here?: boolean; go(f: FolderId | null): void }) {
  const all = p.board.crumbs(p.folder);
  const shown = p.from ? all.slice(all.findIndex(([id]) => id === p.from)) : all;
  return shown.map(([id, name], i) => (
    <Fragment key={id ?? "root"}>
      {i > 0 && " / "}
      {p.here && i === shown.length - 1 ? (
        name
      ) : (
        <button type="button" className="crumb" onClick={() => p.go(id)}>
          {name}
        </button>
      )}
    </Fragment>
  ));
}
