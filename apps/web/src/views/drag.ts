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
  /** The row was let go at `to`; `blocked` when it was dragged further up or down than it may go. */
  drop(from: number, to: number, blocked: "up" | "down" | null): void;
}

/** Rows are `height` tall. */
export function useRowDrag(list: RefObject<HTMLDivElement | null>, height: number, drag: Drag) {
  const latest = useRef(drag);
  useEffect(() => {
    latest.current = drag;
  });
  useEffect(() => {
    const el = list.current!;
    const scroller = el.closest<HTMLElement>(".scroll, .main-scroll")!;
    let dropped = false;
    let press: { id: number; x: number; y: number; row: HTMLElement; timer: ReturnType<typeof setTimeout> } | null = null;
    let lifted: { id: number; row: HTMLElement; rows: HTMLElement[]; from: number; startY: number; startScroll: number; y: number; min: number; max: number; to: number; blocked: "up" | "down" | null; frame: number } | null = null;

    const layout = () => {
      const l = lifted!;
      const dy = l.y - l.startY + scroller.scrollTop - l.startScroll;
      const target = Math.min(Math.max(l.from + Math.round(dy / height), 0), l.rows.length - 1);
      l.to = Math.min(Math.max(target, l.min), l.max);
      l.blocked = target < l.min ? "up" : target > l.max ? "down" : null;
      l.rows.forEach((r, i) => {
        const shift = i === l.from ? dy : l.from < i && i <= l.to ? -height : l.to <= i && i < l.from ? height : 0;
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
      lifted = { id, row, rows, from, startY: y, startScroll: scroller.scrollTop, y, min, max, to: from, blocked: null, frame: 0 };
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

    const down = (e: PointerEvent) => {
      const row = (e.target as Element).closest<HTMLElement>(".row.task");
      if (!e.isPrimary || e.button !== 0 || !row || row.parentElement !== el || lifted) return;
      press = { id: e.pointerId, x: e.clientX, y: e.clientY, row, timer: setTimeout(lift, HOLD_MS) };
    };
    const move = (e: PointerEvent) => {
      if (press?.id === e.pointerId && Math.hypot(e.clientX - press.x, e.clientY - press.y) > SLOP) cancelPress();
      if (lifted?.id !== e.pointerId) return;
      lifted.y = e.clientY;
      layout();
    };
    const up = (e: PointerEvent) => {
      if (press?.id === e.pointerId) cancelPress();
      if (lifted?.id !== e.pointerId) return;
      const { row, rows, from, to, blocked, frame } = lifted;
      lifted = null;
      cancelAnimationFrame(frame);
      delete row.dataset.lifted;
      el.classList.remove("reordering");
      for (const r of rows) r.style.transform = "";
      dropped = true;
      setTimeout(() => (dropped = false), 50);
      if (e.type === "pointerup") latest.current.drop(from, to, blocked);
    };
    // Once a row is up, the finger moves it instead of scrolling the page.
    const touchMove = (e: TouchEvent) => {
      if (lifted) e.preventDefault();
    };
    const menu = (e: Event) => {
      if (press || lifted) e.preventDefault();
    };
    // Letting go of a row is not also a tap on it.
    const click = (e: Event) => {
      if (dropped) e.stopPropagation();
    };
    el.addEventListener("pointerdown", down);
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
    el.addEventListener("touchmove", touchMove, { passive: false });
    el.addEventListener("contextmenu", menu);
    el.addEventListener("click", click, true);
    return () => {
      cancelPress();
      if (lifted) cancelAnimationFrame(lifted.frame);
      el.removeEventListener("pointerdown", down);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      el.removeEventListener("touchmove", touchMove);
      el.removeEventListener("contextmenu", menu);
      el.removeEventListener("click", click, true);
    };
  }, [list, height]);
}
