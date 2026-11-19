// How wide the desktop layout's sidebar and details pane are: dragged by their edges to suit, back to
// the standard with a double click, each device keeping its own (docs/ui.md#布局).

import { type PointerEvent, useState, useSyncExternalStore } from "react";

export type Column = "sidebar" | "pane";

const WIDTHS: Record<Column, { least: number; standard: number; most: number }> = {
  sidebar: { least: 200, standard: 272, most: 400 },
  pane: { least: 320, standard: 400, most: 600 },
};
/** What the list in the middle keeps at least. */
const MIDDLE = 320;

const key = (c: Column) => `nexttodo.width.${c}`;
const within = (w: number, least: number, most: number) => Math.max(least, Math.min(most, w));
const other = (c: Column): Column => (c === "sidebar" ? "pane" : "sidebar");

/** As chosen on this device, else the standard. */
function chosen(c: Column) {
  const { least, standard, most } = WIDTHS[c];
  const w = Number(localStorage.getItem(key(c)) ?? standard);
  return Number.isFinite(w) ? within(w, least, most) : standard;
}

/** A column being dragged, by one pointer: where it started, its width then and as dragged so far, and
 * the room then. */
interface Drag {
  column: Column;
  pointer: number;
  x: number;
  from: number;
  to: number;
  room: number;
}

const onResize = (f: () => void) => {
  addEventListener("resize", f);
  return () => removeEventListener("resize", f);
};
const windowWidth = () => innerWidth;

/** The columns' widths: as chosen, as far as the window leaves room beside the other when it is open,
 * the pane giving way first. One folded away takes no room: the other, held in for it, is back as
 * chosen. And what the edges of those `open` take to be dragged by. */
export function useColumns(open: Record<Column, boolean>) {
  const [widths, setWidths] = useState(() => ({ sidebar: chosen("sidebar"), pane: chosen("pane") }));
  const [drag, setDrag] = useState<Drag | null>(null);
  const room = useSyncExternalStore(onResize, windowWidth) - MIDDLE;
  // Folded away, its edge goes with it and hears no more of the pointer; the window resized, what was
  // dragged to may not fit.
  if (drag && (!open[drag.column] || drag.room !== room)) setDrag(null);
  // As one is dragged, the other has what the window leaves it, as ever: the pane gives way to the
  // sidebar, and widens again as the sidebar narrows.
  const dragged = (c: Column) => (drag?.column === c ? drag.to : undefined);
  const sidebar = dragged("sidebar") ?? within(widths.sidebar, WIDTHS.sidebar.least, room - (open.pane ? WIDTHS.pane.least : 0));
  const shown = { sidebar, pane: dragged("pane") ?? within(widths.pane, WIDTHS.pane.least, room - (open.sidebar ? sidebar : 0)) };

  const edge = (c: Column) => ({
    onPointerDown(e: PointerEvent<HTMLElement>) {
      if (e.button !== 0 || drag) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      setDrag({ column: c, pointer: e.pointerId, x: e.clientX, from: shown[c], to: shown[c], room });
    },
    onPointerMove(e: PointerEvent<HTMLElement>) {
      if (drag?.column !== c || drag.pointer !== e.pointerId) return;
      // The sidebar's edge is on its right, the pane's on its left. Dragged so far, the middle has
      // what it keeps at least, beside the other when it is open: the pane as narrow as it goes, the
      // sidebar as it is, giving the pane nothing.
      const moved = c === "sidebar" ? e.clientX - drag.x : drag.x - e.clientX;
      const beside = !open[other(c)] ? 0 : c === "sidebar" ? WIDTHS.pane.least : shown.sidebar;
      const most = Math.min(WIDTHS[c].most, room - beside);
      setDrag({ ...drag, to: within(drag.from + moved, WIDTHS[c].least, most) });
    },
    onLostPointerCapture(e: PointerEvent<HTMLElement>) {
      if (drag?.column !== c || drag.pointer !== e.pointerId) return;
      setDrag(null);
      // Only what was dragged to is chosen: a width the window held in, not dragged, stays as chosen.
      if (drag.to === drag.from) return;
      setWidths((w) => ({ ...w, [c]: drag.to }));
      localStorage.setItem(key(c), String(drag.to));
    },
    onDoubleClick() {
      localStorage.removeItem(key(c));
      setWidths((w) => ({ ...w, [c]: WIDTHS[c].standard }));
    },
  });

  return { shown, dragging: drag !== null, edge };
}
