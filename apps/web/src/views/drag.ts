// Long-press a row of a rail list, then drag it up or down to put it elsewhere.

import { type RefObject, useEffect, useRef } from "react";

const HOLD_MS = 450;
/** Moving further than this before the hold is over is a scroll or a swipe, not a drag. */
const SLOP = 8;
const EDGE = 56;
const SCROLL_STEP = 8;

export interface Drag {
  /** Where the row at `from` may go, as places in the list without it. */
  range(from: number): { min: number; max: number };
  /** The row at `from` was let go at place `to` in the list without it, which may be further up or
   * down than it may go. */
  drop(from: number, to: number): void;
}

/** Rows are `height` tall. A drag stops without a drop when rows come, go or move meanwhile. */
export function useRowDrag(list: RefObject<HTMLDivElement | null>, height: number, drag: Drag) {
  const latest = useRef(drag);
  useEffect(() => {
    latest.current = drag;
  });
  useEffect(() => {
    const el = list.current!;
    const scroller = el.closest<HTMLElement>(".scroll, .main-scroll")!;
    // The pointer whose press became a drag: letting it go is not also a tap.
    let spent: number | null = null;
    let press: { id: number; x: number; y: number; row: HTMLElement; timer: ReturnType<typeof setTimeout> } | null = null;
    let lifted: { id: number; row: HTMLElement; rows: HTMLElement[]; from: number; startY: number; startScroll: number; y: number; min: number; max: number; target: number; frame: number } | null = null;

    // The other rows make way where the row may go; it follows the pointer anyway.
    const layout = () => {
      const l = lifted!;
      const dy = l.y - l.startY + scroller.scrollTop - l.startScroll;
      l.target = Math.min(Math.max(l.from + Math.round(dy / height), 0), l.rows.length - 1);
      const to = Math.min(Math.max(l.target, l.min), l.max);
      l.rows.forEach((r, i) => {
        const shift = i === l.from ? dy : l.from < i && i <= to ? -height : to <= i && i < l.from ? height : 0;
        r.style.transform = shift ? `translateY(${shift}px)` : "";
      });
    };
    // Near the top or bottom of the scrolling area, keep scrolling while the row is held there.
    const scroll = () => {
      const l = lifted!;
      const box = scroller.getBoundingClientRect();
      const step = l.y < box.top + EDGE ? -SCROLL_STEP : l.y > box.bottom - EDGE ? SCROLL_STEP : 0;
      if (step) {
        scroller.scrollTop += step;
        layout();
      }
      l.frame = requestAnimationFrame(scroll);
    };
    const lift = () => {
      const { id, y, row } = press!;
      press = null;
      const rows = [...el.querySelectorAll<HTMLElement>(":scope > .row")];
      const from = rows.indexOf(row);
      const { min, max } = latest.current.range(from);
      lifted = { id, row, rows, from, startY: y, startScroll: scroller.scrollTop, y, min, max, target: from, frame: 0 };
      row.dataset.lifted = "";
      el.classList.add("reordering");
      row.setPointerCapture(id);
      navigator.vibrate?.(10);
      lifted.frame = requestAnimationFrame(scroll);
    };
    const cancelPress = () => {
      if (press) clearTimeout(press.timer);
      press = null;
    };
    /** Puts the row back; `drop` when it was let go, rather than the drag cut short. */
    const end = (drop: boolean) => {
      cancelPress();
      if (!lifted) return;
      const { id, row, rows, from, target, frame } = lifted;
      lifted = null;
      spent = id;
      cancelAnimationFrame(frame);
      delete row.dataset.lifted;
      el.classList.remove("reordering");
      for (const r of rows) r.style.transform = "";
      if (drop) latest.current.drop(from, target);
    };
    const rowsChanged = new MutationObserver(() => end(false));
    rowsChanged.observe(el, { childList: true });

    const down = (e: PointerEvent) => {
      const row = (e.target as Element).closest<HTMLElement>(".row.task");
      if (!e.isPrimary || e.button !== 0 || !row || row.parentElement !== el || lifted) return;
      spent = null;
      press = { id: e.pointerId, x: e.clientX, y: e.clientY, row, timer: setTimeout(lift, HOLD_MS) };
    };
    // Moves and the release are heard wherever they happen, also outside the list.
    const move = (e: PointerEvent) => {
      if (press?.id === e.pointerId && Math.hypot(e.clientX - press.x, e.clientY - press.y) > SLOP) cancelPress();
      if (lifted?.id !== e.pointerId) return;
      lifted.y = e.clientY;
      layout();
    };
    const up = (e: PointerEvent) => {
      if (press?.id === e.pointerId || lifted?.id === e.pointerId) end(e.type === "pointerup");
      // The click comes right after.
      if (spent === e.pointerId) setTimeout(() => (spent = null), 50);
    };
    const blur = () => end(false);
    // Once a row is up, the finger moves it instead of scrolling the page.
    const touchMove = (e: TouchEvent) => {
      if (lifted) e.preventDefault();
    };
    const menu = (e: Event) => {
      if (press || lifted) e.preventDefault();
    };
    const click = (e: Event) => {
      if (spent !== null) e.stopPropagation();
    };
    el.addEventListener("pointerdown", down);
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
    addEventListener("pointercancel", up);
    addEventListener("blur", blur);
    el.addEventListener("touchmove", touchMove, { passive: false });
    el.addEventListener("contextmenu", menu);
    el.addEventListener("click", click, true);
    return () => {
      cancelPress();
      if (lifted) cancelAnimationFrame(lifted.frame);
      rowsChanged.disconnect();
      el.removeEventListener("pointerdown", down);
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", up);
      removeEventListener("pointercancel", up);
      removeEventListener("blur", blur);
      el.removeEventListener("touchmove", touchMove);
      el.removeEventListener("contextmenu", menu);
      el.removeEventListener("click", click, true);
    };
  }, [list, height]);
}
