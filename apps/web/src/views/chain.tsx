// What follows a task in “接下来”, unfolded under its row (docs/ui.md#接下来里的后续).

import { useState } from "react";
import { flushSync } from "react-dom";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { dueAfterLabel, formatDue, startLabel } from "../domain/dates";
import { plainRail, type RailRow, railLayout } from "../domain/rail";
import { useKept } from "../kept";
import { useWide } from "../ui/hooks";
import { MAX_LANES, type RowActions, dueClass } from "./rows";

/** How many are listed before “还有 N 项”. */
const MOST = 8;
const ITEM = 40;

/** How many unfolded tasks are kept. Ended ones stay, so that undoing or reopening one brings it back
 * unfolded, until this many others were unfolded since. */
const KEPT = 50;

const parse = (saved: string | null): TaskId[] => (saved ? JSON.parse(saved) : []);

/** Which tasks have what follows them unfolded, kept on this device for each user. */
export function useChains(board: Board) {
  const key = `nexttodo.openChains:${board.me}`;
  const [kept, keep] = useKept(key);
  const toggle = (t: Task) => {
    // As kept now: another window may have changed it since this one drew.
    const was = parse(localStorage.getItem(key));
    const rest = was.filter((id) => id !== t.id);
    keep(JSON.stringify(rest.length < was.length ? rest : [...rest, t.id].slice(-KEPT)));
  };
  return { open: new Set(parse(kept)), toggle };
}

/** The open tasks after `task`, the rail going on from its circle in the row above. */
export function Chain({ task: t, board, now, selected, actions }: { task: Task; board: Board; now: Date; selected: TaskId | null; actions: RowActions }) {
  const [all, setAll] = useState(false);
  const wide = useWide();
  const after = board.order([t, ...board.openAfter(t)], () => false).slice(1);
  const shown = all ? after : after.slice(0, MOST);
  // The task's own row is above; its lines leave from the top, bending within the first item's height.
  const row = (i: number): RailRow =>
    i === 0 ? { top: 0, mid: 0, bottom: 12, clear: 0 } : { top: (i - 1) * ITEM, mid: (i - 1) * ITEM + ITEM / 2, bottom: i * ITEM, clear: 8 };
  const layout = railLayout([t, ...shown], board, MAX_LANES[wide ? "wide" : "narrow"], row);
  const rail = layout ?? { ...plainRail, x: () => plainRail.x };
  const chained = new Set(after.map((a) => a.id));
  const unlocks = new Set(board.unlocks(t).map((a) => a.id));
  const note = (a: Task) => {
    if (unlocks.has(a.id)) return <span className="chain-note m-unlock">完成后可做</span>;
    const others = board.before(a).filter((p) => p.id !== t.id && !chained.has(p.id) && board.holdsUp(p));
    if (others.length) return <span className="chain-note">另需{others.length === 1 ? `“${others[0].title}”` : ` ${others.length} 项`}</span>;
    if (board.notStarted(a)) return <span className="chain-note">{startLabel(a.start!, a.start_time, now)}</span>;
    if (a.due) return <span className={`chain-note ${dueClass(a, now)}`}>{formatDue(a.due, now)}</span>;
    if (a.due_after) return <span className="chain-note">{dueAfterLabel(a.due_after)}</span>;
    return null;
  };
  return (
    <div className="chain" style={{ "--gutter": `${rail.gutter}px` }}>
      <svg className="rail" width={rail.gutter} height={shown.length * ITEM}>
        {layout ? (
          layout.paths.map((p) => <path key={p.key} d={p.d} className={p.live ? "live" : undefined} />)
        ) : (
          // Without the lines, still on from the row, ready as all in “接下来” are, to the first, which
          // always follows it directly.
          <path d={`M${plainRail.x} 0 V${ITEM / 2}`} className="live" />
        )}
        {shown.map((a, i) => (
          <circle key={a.id} cx={rail.x(a)} cy={i * ITEM + ITEM / 2} r={6} className={board.status(a)} />
        ))}
      </svg>
      {shown.map((a) => (
        <button key={a.id} type="button" className={`chain-item ${board.status(a)}${selected === a.id ? " selected" : ""}`} onClick={() => actions.open(a)}>
          <span className="chain-title">{a.title}</span>
          {note(a)}
        </button>
      ))}
      {shown.length < after.length && (
        <button
          type="button"
          className="chain-more"
          onClick={(e) => {
            const list = e.currentTarget.parentElement!;
            flushSync(() => setAll(true));
            // The button is gone: on to the first of those it brought.
            list.querySelectorAll<HTMLElement>(".chain-item")[MOST].focus();
          }}
        >
          还有 {after.length - shown.length} 项
        </button>
      )}
    </div>
  );
}
