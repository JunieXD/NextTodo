// Dragging rows to put them elsewhere (docs/ui.md#拖动): up or down a list, into a folder, about the
// folder tree. A finger long-presses a row first, so as not to take a scroll or a swipe for a drag; a
// mouse just drags it.

import { type RefObject, useEffect, useRef } from "react";
import type { FolderId } from "../api/types/FolderId";

const HOLD_MS = 450;
/** A finger moving further than this before the hold is over scrolls or swipes; a mouse moving this
 * far drags. */
const SLOP = 8;
const EDGE = 56;
const SCROLL_STEP = 8;
/** How long a folder folded away is held over before it unfolds. */
const UNFOLD_MS = 700;

/** The folder a row standing for one takes in: its `data-into`, empty for the root. */
const intoOf = (row: HTMLElement): FolderId | null => row.dataset.into || null;

/** The row standing for a folder under the pointer, other than the one `lifted`. */
function intoUnder(x: number, y: number, lifted: HTMLElement): HTMLElement | null {
  return (
    document
      .elementsFromPoint(x, y)
      .find((e) => !lifted.contains(e))
      ?.closest<HTMLElement>("[data-into]") ?? null
  );
}

/** How far to scroll `scroller` with the pointer held near its top or bottom, over it. */
function edgeStep(scroller: HTMLElement, x: number, y: number): number {
  const box = scroller.getBoundingClientRect();
  if (x < box.left || x > box.right) return 0;
  return y < box.top + EDGE ? -SCROLL_STEP : y > box.bottom - EDGE ? SCROLL_STEP : 0;
}

/** Lights up the row a drag would go into, `now`, rather than `was`. */
function light(was: HTMLElement | null, now: HTMLElement | null) {
  if (was === now) return;
  if (was) delete was.dataset.drop;
  if (now) now.dataset.drop = "";
}

interface Gesture {
  /** Whether a press on `row` may lift it. */
  can(row: HTMLElement): boolean;
  lift(row: HTMLElement, x: number, y: number): void;
  move(x: number, y: number): void;
  /** Let go (`drop`), or cut short. */
  end(drop: boolean): void;
}

/** Hears a press on one of `rows` in `el` becoming a drag, and the drag's moves and end, wherever they
 * happen. Returns how to cut the drag short, and to stop hearing. */
function hearDrags(el: HTMLElement, rows: string, g: Gesture): { cancel(): void; stop(): void } {
  // The pointer whose press became a drag: letting it go is not also a tap.
  let spent: number | null = null;
  // A mouse's press has no timer: it drags as soon as it moves.
  let press: { id: number; x: number; y: number; row: HTMLElement; timer?: ReturnType<typeof setTimeout> } | null = null;
  let lifted: number | null = null;
  const lift = () => {
    const { id, x, y, row } = press!;
    press = null;
    lifted = id;
    row.setPointerCapture(id);
    navigator.vibrate?.(10);
    g.lift(row, x, y);
  };
  const cancelPress = () => {
    if (press) clearTimeout(press.timer);
    press = null;
  };
  const end = (drop: boolean) => {
    cancelPress();
    if (lifted === null) return;
    spent = lifted;
    lifted = null;
    g.end(drop);
  };
  const down = (e: PointerEvent) => {
    const row = (e.target as Element).closest<HTMLElement>(rows);
    // With Control held, a Mac takes the click for a right click: it opens the row's menu.
    if (!e.isPrimary || e.button !== 0 || e.ctrlKey || !row || !el.contains(row) || lifted !== null || !g.can(row)) return;
    spent = null;
    press = { id: e.pointerId, x: e.clientX, y: e.clientY, row, timer: e.pointerType === "mouse" ? undefined : setTimeout(lift, HOLD_MS) };
  };
  const move = (e: PointerEvent) => {
    if (press?.id === e.pointerId && Math.hypot(e.clientX - press.x, e.clientY - press.y) > SLOP) {
      if (press.timer === undefined) lift();
      else cancelPress();
    }
    if (lifted === e.pointerId) g.move(e.clientX, e.clientY);
  };
  const up = (e: PointerEvent) => {
    if (press?.id === e.pointerId || lifted === e.pointerId) end(e.type === "pointerup");
    // The click comes right after.
    if (spent === e.pointerId) setTimeout(() => (spent = null), 50);
  };
  const blur = () => end(false);
  const key = (e: KeyboardEvent) => {
    if (lifted === null || e.key !== "Escape") return;
    e.preventDefault();
    end(false);
  };
  // Once a row is up, the finger moves it instead of scrolling the page.
  const touchMove = (e: TouchEvent) => {
    if (lifted !== null) e.preventDefault();
  };
  const menu = (e: Event) => {
    if (press || lifted !== null) e.preventDefault();
  };
  const click = (e: Event) => {
    if (spent !== null) e.stopPropagation();
  };
  el.addEventListener("pointerdown", down);
  // Moves and the release are heard wherever they happen, also outside `el`.
  addEventListener("pointermove", move);
  addEventListener("pointerup", up);
  addEventListener("pointercancel", up);
  addEventListener("blur", blur);
  addEventListener("keydown", key);
  el.addEventListener("touchmove", touchMove, { passive: false });
  el.addEventListener("contextmenu", menu);
  el.addEventListener("click", click, true);
  return {
    cancel: () => end(false),
    stop() {
      cancelPress();
      el.removeEventListener("pointerdown", down);
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", up);
      removeEventListener("pointercancel", up);
      removeEventListener("blur", blur);
      removeEventListener("keydown", key);
      el.removeEventListener("touchmove", touchMove);
      el.removeEventListener("contextmenu", menu);
      el.removeEventListener("click", click, true);
    },
  };
}

export interface Drag {
  /** Where the row at `from` may go, as places in the list without it. */
  range(from: number): { min: number; max: number };
  /** The row at `from` was let go at place `to` in the list without it, which may be further up or
   * down than it may go. */
  drop(from: number, to: number): void;
  /** Whether the row at `from` may go into `folder`, that of a row standing for one (`data-into`):
   * one elsewhere under the pointer, or one of these rows, by its middle half. */
  canInto(from: number, folder: FolderId | null): boolean;
  /** The row at `from` was let go into `folder`. */
  into(from: number, folder: FolderId | null): void;
  /** Rows stay put, as while tasks are being chosen. */
  off?: boolean;
}

/** Drags the children of `list` that match `rows`, all as tall as each other. A drag stops without
 * a drop when rows come, go or move meanwhile. */
export function useRowDrag(list: RefObject<HTMLDivElement | null>, rows: string, drag: Drag) {
  const latest = useRef(drag);
  useEffect(() => {
    latest.current = drag;
  });
  useEffect(() => {
    const el = list.current!;
    const scroller = el.closest<HTMLElement>(".scroll, .main-scroll")!;
    let l: {
      row: HTMLElement;
      rows: HTMLElement[];
      height: number;
      from: number;
      startY: number;
      startScroll: number;
      x: number;
      y: number;
      min: number;
      max: number;
      /** The rows take others in, as folders do: then the top or bottom quarter of one puts the row
       * before or after it. */
      nests: boolean;
      target: number;
      into: HTMLElement | null;
      /** What follows the pointer in place of the row, which its group would cut off past its edges. */
      ghost: HTMLElement;
      frame: number;
    } | null = null;

    // The other rows make way where the row may go, or stay put while it is over a folder it would go
    // into; its copy follows the pointer anyway.
    const layout = () => {
      const s = l!;
      const dy = s.y - s.startY + scroller.scrollTop - s.startScroll;
      // Which row the lifted row's middle is over, as the rows were, and how far down that row.
      const at = s.from + 0.5 + dy / s.height;
      const i = Math.floor(at);
      const part = at - i;
      const elsewhere = intoUnder(s.x, s.y, s.row);
      const here = s.nests && i !== s.from && part > 0.25 && part < 0.75 ? s.rows[i] : undefined;
      const into = elsewhere && !el.contains(elsewhere) ? elsewhere : here && "into" in here.dataset ? here : null;
      const taken = into && latest.current.canInto(s.from, intoOf(into)) ? into : null;
      light(s.into, taken);
      s.into = taken;
      s.ghost.classList.toggle("into", !!taken);
      s.ghost.style.transform = `translateY(${s.y - s.startY}px)`;
      // Before row `i` by its top half, after it by its bottom half; as places in the list without the
      // row lifted, those below it are one further up.
      const beside = i === s.from || !s.nests ? i : i - Number(i > s.from) + Number(part >= 0.5);
      s.target = Math.min(Math.max(beside, 0), s.rows.length - 1);
      const to = taken ? s.from : Math.min(Math.max(s.target, s.min), s.max);
      s.rows.forEach((r, k) => {
        const shift = s.from < k && k <= to ? -s.height : to <= k && k < s.from ? s.height : 0;
        r.style.transform = shift ? `translateY(${shift}px)` : "";
      });
    };
    // Near the top or bottom of the scrolling area, keep scrolling while the row is held there.
    const scroll = () => {
      const step = edgeStep(scroller, l!.x, l!.y);
      if (step) {
        scroller.scrollTop += step;
        layout();
      }
      l!.frame = requestAnimationFrame(scroll);
    };
    const drags = hearDrags(el, rows, {
      can: (row) => row.parentElement === el && !latest.current.off,
      lift(row, x, y) {
        const all = [...el.querySelectorAll<HTMLElement>(`:scope > ${rows}`)];
        const from = all.indexOf(row);
        const { min, max } = latest.current.range(from);
        const nests = all.some((r) => "into" in r.dataset);
        // The row in a copy of the list, which its look depends on, over the app.
        const ghost = el.cloneNode() as HTMLElement;
        ghost.classList.add("drag-ghost");
        ghost.ariaHidden = "true";
        ghost.append(row.cloneNode(true));
        const box = row.getBoundingClientRect();
        Object.assign(ghost.style, { left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px` });
        el.closest(".app")!.append(ghost);
        l = { row, rows: all, height: row.offsetHeight, from, startY: y, startScroll: scroller.scrollTop, x, y, min, max, nests, target: from, into: null, ghost, frame: 0 };
        row.dataset.lifted = "";
        el.classList.add("reordering");
        l.frame = requestAnimationFrame(scroll);
      },
      move(x, y) {
        l!.x = x;
        l!.y = y;
        layout();
      },
      end(drop) {
        const { row, rows, from, target, into, ghost, frame } = l!;
        l = null;
        cancelAnimationFrame(frame);
        ghost.remove();
        delete row.dataset.lifted;
        light(into, null);
        el.classList.remove("reordering");
        for (const r of rows) r.style.transform = "";
        if (!drop) return;
        if (into) latest.current.into(from, intoOf(into));
        else latest.current.drop(from, target);
      },
    });
    const rowsChanged = new MutationObserver(() => drags.cancel());
    rowsChanged.observe(el, { childList: true });
    return () => {
      if (l) {
        cancelAnimationFrame(l.frame);
        l.ghost.remove();
      }
      rowsChanged.disconnect();
      drags.stop();
    };
  }, [list, rows]);
}

export interface TreeDrag {
  /** Whether `folder` may go into `parent`: not into itself, nor anywhere inside it. */
  canPlace(folder: FolderId, parent: FolderId | null): boolean;
  /** `folder` was let go in `parent`, before the folder `before` there, or last. */
  place(folder: FolderId, parent: FolderId | null, before: FolderId | null): void;
  unfold(folder: FolderId | null): void;
}

/** Where a folder dragged about the tree would go: into a row, lit up; or among the rows, where a line
 * shows, at a depth. */
type Spot = { parent: FolderId | null; before: FolderId | null } & ({ into: HTMLElement } | { line: { top: number; depth: number } });

/** Drags the folders of the tree in `tree`: its rows `.sb-item[data-into]` (empty for the root, which
 * stays put), with `data-parent`, `data-depth` and, folded away with folders inside, `data-closed`.
 * Let go on a row's middle half, a folder goes into it; between two rows, before the one below, at its
 * depth; under them all, last at the top. The rows stay put, the one dragged follows the pointer up
 * or down. */
export function useTreeDrag(tree: RefObject<HTMLElement | null>, drag: TreeDrag) {
  const latest = useRef(drag);
  useEffect(() => {
    latest.current = drag;
  });
  useEffect(() => {
    const el = tree.current!;
    const line = document.createElement("div");
    line.className = "sb-line";
    let s: {
      row: HTMLElement;
      folder: FolderId;
      parent: FolderId | null;
      startY: number;
      startScroll: number;
      x: number;
      y: number;
      spot: Spot | null;
      unfold?: ReturnType<typeof setTimeout>;
      frame: number;
    } | null = null;

    /** Where the folder would go, let go at the pointer over `rows`, whose bottom is `end` down the
     * tree; nowhere it may not. */
    const spotAt = (rows: HTMLElement[], end: number, x: number, y: number): Spot | null => {
      const box = el.getBoundingClientRect();
      if (x < box.left || x > box.right) return null;
      const row = intoUnder(x, y, s!.row);
      const atEnd: Spot = { parent: null, before: null, line: { top: end, depth: 1 } };
      let spot: Spot;
      if (row && el.contains(row)) {
        const r = row.getBoundingClientRect();
        const part = (y - r.top) / r.height;
        const below = part <= 0.25 ? row : rows[rows.indexOf(row) + 1];
        // Nothing goes before the root: it takes the folder in anywhere.
        if (!row.dataset.into || (part > 0.25 && part < 0.75)) spot = { parent: intoOf(row), before: null, into: row };
        else if (below) spot = { parent: below.dataset.parent || null, before: intoOf(below), line: { top: below.offsetTop, depth: Number(below.dataset.depth) } };
        else spot = atEnd;
      } else if (y - box.top - el.clientTop + el.scrollTop > end) spot = atEnd;
      else return null;
      // Into the folder it is in already, it would only go last: that is not going into it. Right
      // before itself, it stays where it is.
      if ("into" in spot ? spot.parent === s!.parent : spot.before === s!.folder) return null;
      return latest.current.canPlace(s!.folder, spot.parent) ? spot : null;
    };
    const show = (spot: Spot | null) => {
      const was = s!.spot;
      const into = spot && "into" in spot ? spot.into : null;
      if (!was || !("into" in was) || was.into !== into) {
        clearTimeout(s!.unfold);
        if (into && "closed" in into.dataset) s!.unfold = setTimeout(() => latest.current.unfold(intoOf(into)), UNFOLD_MS);
      }
      light(was && "into" in was ? was.into : null, into);
      if (spot && "line" in spot) {
        line.style.top = `${spot.line.top}px`;
        line.style.setProperty("--depth", String(spot.line.depth));
        el.append(line);
      } else line.remove();
      s!.spot = spot;
    };
    // Each frame: near the top or bottom, scroll on; the folder follows the pointer, and the rows may
    // have moved under it, a folder unfolding.
    const frame = () => {
      const { row, x, y } = s!;
      const step = edgeStep(el, x, y);
      if (step) el.scrollTop += step;
      const rows = [...el.querySelectorAll<HTMLElement>(".sb-item[data-into]")];
      // Where the rows end, as laid out: the one dragged moves, not its place.
      const last = rows.at(-1)!;
      const end = last.offsetTop + last.offsetHeight;
      // Not much further than the rows, which would make the tree longer, to scroll on and on.
      const dy = Math.min(y - s!.startY + el.scrollTop - s!.startScroll, end - row.offsetTop);
      row.style.transform = `translateY(${dy}px)`;
      show(spotAt(rows, end, x, y));
      s!.frame = requestAnimationFrame(frame);
    };
    const drags = hearDrags(el, ".sb-item[data-into]", {
      can: (row) => !!row.dataset.into,
      lift(row, x, y) {
        s = { row, folder: intoOf(row)!, parent: row.dataset.parent || null, startY: y, startScroll: el.scrollTop, x, y, spot: null, frame: 0 };
        row.dataset.lifted = "";
        s.frame = requestAnimationFrame(frame);
      },
      move(x, y) {
        s!.x = x;
        s!.y = y;
      },
      end(drop) {
        const { row, folder, spot, unfold } = s!;
        show(null);
        cancelAnimationFrame(s!.frame);
        clearTimeout(unfold);
        s = null;
        delete row.dataset.lifted;
        row.style.transform = "";
        if (drop && spot) latest.current.place(folder, spot.parent, spot.before);
      },
    });
    return () => {
      if (s) {
        cancelAnimationFrame(s.frame);
        clearTimeout(s.unfold);
      }
      line.remove();
      drags.stop();
    };
  }, [tree]);
}
