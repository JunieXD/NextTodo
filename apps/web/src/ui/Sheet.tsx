// The phone's detail panel: half open (the list behind stays usable) or fully open, dragged from
// anywhere. In the content a drag starts once the content is scrolled to the top, or upward from
// half open; a quick flick down goes from full to half, and from half to closed.

import { type ReactNode, useEffect, useRef, useState } from "react";
import { useBack } from "./back";

export function Sheet({ label, onClose, children }: { label: string; onClose(): void; children: ReactNode }) {
  const [full, setFull] = useState(false);
  const sheet = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const state = useRef({ full, onClose });
  useBack(onClose);
  useEffect(() => {
    state.current = { full, onClose };
  });

  useEffect(() => {
    const el = sheet.current!;
    let drag: { startY: number; startTop: number; fullTop: number; top: number | null; v: number; lastY: number; lastT: number } | null = null;
    let touch: { y: number; mode: "drag" | "scroll" | null } | null = null;

    const start = (y: number) => {
      drag = { startY: y, startTop: el.getBoundingClientRect().top, fullTop: innerHeight - el.offsetHeight, top: null, v: 0, lastY: y, lastT: performance.now() };
      el.classList.add("dragging");
    };
    const move = (y: number) => {
      if (!drag) return;
      const now = performance.now();
      drag.v = (y - drag.lastY) / Math.max(1, now - drag.lastT);
      drag.lastY = y;
      drag.lastT = now;
      drag.top = Math.max(drag.fullTop, drag.startTop + y - drag.startY);
      el.style.transform = `translateY(${drag.top - drag.fullTop}px)`;
    };
    const end = () => {
      if (!drag) return;
      const { top, v } = drag;
      drag = null;
      el.classList.remove("dragging");
      el.style.transform = "";
      if (top === null) return;
      const { full, onClose } = state.current;
      if (v > 0.6) {
        if (full) setFull(false);
        else onClose();
      } else if (v < -0.6) setFull(true);
      else if (top > innerHeight * 0.72) onClose();
      else setFull(top < innerHeight * 0.3);
    };

    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Element;
      if (e.pointerType === "touch" || !target.closest("[data-drag]") || target.closest("button, input, textarea")) return;
      start(e.clientY);
    };
    const onPointerMove = (e: PointerEvent) => e.pointerType !== "touch" && move(e.clientY);
    const onPointerUp = (e: PointerEvent) => e.pointerType !== "touch" && end();
    const onTouchStart = (e: TouchEvent) => {
      if (e.touches.length !== 1 || (e.target as Element).closest("textarea, input")) return;
      touch = { y: e.touches[0].clientY, mode: null };
    };
    const onTouchMove = (e: TouchEvent) => {
      if (!touch) return;
      const y = e.touches[0].clientY;
      const dy = y - touch.y;
      if (!touch.mode) {
        if (Math.abs(dy) < 3) return;
        const content = scroller.current!;
        const pull = !content.contains(e.target as Node) || (dy > 0 && content.scrollTop <= 0) || (dy < 0 && !state.current.full);
        touch.mode = pull ? "drag" : "scroll";
        if (pull) start(touch.y);
      }
      if (touch.mode === "drag") {
        e.preventDefault();
        move(y);
      }
    };
    const onTouchEnd = () => {
      if (touch?.mode === "drag") end();
      touch = null;
    };

    el.addEventListener("pointerdown", onPointerDown);
    addEventListener("pointermove", onPointerMove);
    addEventListener("pointerup", onPointerUp);
    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("touchmove", onTouchMove, { passive: false });
    el.addEventListener("touchend", onTouchEnd);
    return () => {
      el.removeEventListener("pointerdown", onPointerDown);
      removeEventListener("pointermove", onPointerMove);
      removeEventListener("pointerup", onPointerUp);
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchmove", onTouchMove);
      el.removeEventListener("touchend", onTouchEnd);
    };
  }, []);

  const mode = full ? "full" : "half";
  return (
    <>
      <div className={`scrim ${mode} enter`} onClick={onClose} />
      <div ref={sheet} className={`sheet ${mode} enter`} role="dialog" aria-label={label}>
        <div className="sheet-grab" data-drag>
          <span className="handle" />
        </div>
        <div ref={scroller} className="sheet-scroll">
          {children}
        </div>
      </div>
    </>
  );
}
