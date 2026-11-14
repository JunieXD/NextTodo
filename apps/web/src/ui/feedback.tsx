// Snackbar, confirmation and input dialogs, modal panels and tips, all inside the app's root element
// so that the phone and desktop styles apply to them.

import { Dialog, Tooltip } from "radix-ui";
import { X } from "lucide-react";
import { type ReactElement, type ReactNode, type RefObject, createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ApiError } from "../api/http";
import type { Checked } from "../domain/text";
import { useBack } from "./back";
import { Icon } from "./icons";
import { useLeave } from "./leave";

export const RootContext = createContext<HTMLElement | null>(null);

/** What to tell the user of `e`, which made `what` fail, as docs/ui.md#文案 has it: that `what` failed,
 * then the server's reason; a message of the app's native part or of this page, written for the user
 * in Chinese as all such are, says it all. Others go to the console, where they help: to the user
 * they mean little. */
export function forUser(what: string, e: unknown): string {
  if (e instanceof ApiError) return `${what}失败：${e.message}`;
  const message = typeof e === "string" ? e : (e as { message?: unknown } | null)?.message;
  if (typeof message === "string" && /[\u4e00-\u9fff]/.test(message)) return message;
  console.warn(what, e);
  return `${what}失败，请稍后重试`;
}

interface Snack {
  text: string;
  action?: string;
  onAction?: () => void;
  ms?: number;
}

interface Confirm {
  title: string;
  body: string;
  /** Text to read in full, such as the other side's version, in a box that scrolls. */
  detail?: string;
  ok: string;
  /** The other answer, which closing the dialog gives too; "取消" unless said. */
  cancel?: string;
  danger?: boolean;
}

interface Prompt {
  title: string;
  label: string;
  value?: string;
  placeholder?: string;
  ok: string;
  /** Cleans the input or says what is wrong with it. */
  check: (s: string) => Checked;
}

interface Feedback {
  snack(s: Snack | string): void;
  confirm(c: Confirm): Promise<boolean>;
  prompt(p: Prompt): Promise<string | null>;
}

const FeedbackContext = createContext<Feedback>(null!);

export const useFeedback = () => useContext(FeedbackContext);

type Open = (Confirm & { kind: "confirm"; resolve(ok: boolean): void }) | (Prompt & { kind: "prompt"; resolve(value: string | null): void });

/** `outside`: the app has been opened from outside, to quick add or to a task. */
export function FeedbackProvider({ outside, children }: { outside: boolean; children: ReactNode }) {
  const [snack, setSnack] = useState<(Snack & { key: number }) | null>(null);
  const [open, setOpen] = useState<Open | null>(null);
  /** Answers the dialog open as cancelled. */
  const cancel = useRef<() => void>(undefined);
  /** `outside` as last seen: a dialog asked for meanwhile is answered as cancelled at once. */
  const away = useRef(outside);
  const feedback = useMemo<Feedback>(
    () => ({
      snack(s) {
        setSnack({ ...(typeof s === "string" ? { text: s } : s), key: Date.now() });
      },
      // A dialog asked for while another is open takes its place, which is answered as cancelled.
      confirm: (c) =>
        new Promise((resolve) => {
          cancel.current?.();
          if (away.current) return resolve(false);
          cancel.current = () => resolve(false);
          setOpen({ ...c, kind: "confirm", resolve });
        }),
      prompt: (p) =>
        new Promise((resolve) => {
          cancel.current?.();
          if (away.current) return resolve(null);
          cancel.current = () => resolve(null);
          setOpen({ ...p, kind: "prompt", resolve });
        }),
    }),
    [],
  );
  // Opened from outside, the dialog makes way, as cancelled. It leaves in this very render, so that
  // its focus trap does not keep the focus from where it has gone, such as to quick add.
  if (outside && open) setOpen(null);
  useEffect(() => {
    away.current = outside;
    if (outside) cancel.current?.();
  }, [outside]);
  const close = () => setSnack(null);
  const act = () => {
    close();
    snack?.onAction?.();
  };
  return (
    <FeedbackContext value={feedback}>
      {children}
      {snack && <Snackbar key={snack.key} text={snack.text} action={snack.action} ms={snack.ms ?? 5000} onAction={act} onClose={close} />}
      {open?.kind === "confirm" && <ConfirmDialog {...open} close={(ok) => (setOpen(null), open.resolve(ok))} />}
      {open?.kind === "prompt" && <PromptDialog {...open} close={(value) => (setOpen(null), open.resolve(value))} />}
    </FeedbackContext>
  );
}

/** A new one comes in over the one it replaces as that fades. It closes after `ms`, or before with
 * the ×, or swiped away sideways, as a notification is. */
function Snackbar({ text, action, ms, onAction, onClose }: { text: string; action?: string; ms: number; onAction(): void; onClose(): void }) {
  const el = useRef<HTMLDivElement>(null);
  useLeave([el]);
  useGoing(el, ms, onClose);
  return (
    <div ref={el} className="snackbar enter" role="status">
      <span>{text}</span>
      {action && <button onClick={onAction}>{action}</button>}
      <Tip label="关闭">
        <button className="snack-close" onClick={onClose}>
          <Icon of={X} size={18} />
        </button>
      </Tip>
    </div>
  );
}

/** How far a swipe takes the snackbar away, as a part of its width; or how fast it goes outwards as
 * the finger lifts, in px per ms, over the moves of the last `RECENT` ms since it last turned. */
const AWAY = 1 / 3;
const FLICK = 0.5;
const RECENT = 100;

/** Calls `close` `ms` after the snackbar `el` shows, or once it is swiped away: a finger moving it
 * sideways takes it along, fading; let go far or fast enough, it goes on out, otherwise it comes
 * back, as it does when the system takes the touch away. The time waits while it is held, and starts
 * again as it comes back, as on Android. Its buttons do not take the end of a swipe for a tap. */
function useGoing(ref: RefObject<HTMLElement | null>, ms: number, close: () => void) {
  const latest = useRef(close);
  useEffect(() => {
    latest.current = close;
  });
  useEffect(() => {
    const el = ref.current!;
    let s: { x: number; y: number; dx: number; active: boolean; moves: { dx: number; at: number }[] } | null = null;
    let swiped = false;
    // Unmounted, it may still be on screen, leaving: it is no longer the one `close` closes.
    let mounted = true;
    let timer = setTimeout(() => latest.current(), ms);
    const down = () => (swiped = false);
    const start = (e: TouchEvent) => {
      if (e.touches.length !== 1) return;
      s = { x: e.touches[0].clientX, y: e.touches[0].clientY, dx: 0, active: false, moves: [{ dx: 0, at: e.timeStamp }] };
    };
    const move = (e: TouchEvent) => {
      if (!s) return;
      const dx = e.touches[0].clientX - s.x;
      const last = s.moves.at(-1)!;
      // Turned back, it goes as fast as it has since: flung out and pulled back, it is not flung.
      if ((dx - last.dx) * (last.dx - s.moves[0].dx) < 0) s.moves = [last];
      s.moves.push({ dx, at: e.timeStamp });
      if (!s.active) {
        if (Math.abs(e.touches[0].clientY - s.y) > 10) return void (s = null);
        if (Math.abs(dx) < 12) return;
        s.active = swiped = true;
        clearTimeout(timer);
        el.classList.add("swiping");
      }
      s.dx = dx;
      el.style.transform = `translateX(${dx}px)`;
      el.style.opacity = String(Math.max(0, 1 - Math.abs(dx) / el.offsetWidth));
    };
    const end = (e: TouchEvent) => {
      const ended = s;
      s = null;
      if (!ended?.active) return;
      el.classList.remove("swiping");
      const width = el.offsetWidth;
      // How fast it went at the end, whatever came before: held a while, it can still be flung.
      const [from] = ended.moves.filter((m) => e.timeStamp - m.at <= RECENT);
      const speed = from ? (Math.sign(ended.dx) * (ended.dx - from.dx)) / Math.max(e.timeStamp - from.at, 1) : 0;
      // Taken away by the system, the touch was not let go of.
      if (e.type === "touchend" && (Math.abs(ended.dx) > width * AWAY || speed > FLICK)) {
        el.style.transform = `translateX(${Math.sign(ended.dx) * width}px)`;
        el.style.opacity = "0";
        // Gone once it is out, as `useLeave` waits: a transition cut short ends too.
        void Promise.allSettled(el.getAnimations().map((a) => a.finished)).then(() => mounted && latest.current());
      } else {
        el.style.transform = "";
        el.style.opacity = "";
        timer = setTimeout(() => latest.current(), ms);
      }
    };
    const click = (e: MouseEvent) => swiped && e.stopPropagation();
    el.addEventListener("pointerdown", down);
    el.addEventListener("touchstart", start, { passive: true });
    el.addEventListener("touchmove", move, { passive: true });
    el.addEventListener("touchend", end);
    el.addEventListener("touchcancel", end);
    el.addEventListener("click", click, true);
    return () => {
      mounted = false;
      clearTimeout(timer);
      el.removeEventListener("pointerdown", down);
      el.removeEventListener("touchstart", start);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", end);
      el.removeEventListener("touchcancel", end);
      el.removeEventListener("click", click, true);
    };
  }, [ref, ms]);
}

function DialogFrame({ title, close, children }: { title: string; close(): void; children: ReactNode }) {
  const root = useContext(RootContext);
  const scrim = useRef<HTMLDivElement>(null);
  useLeave([scrim]);
  useBack("dialog", close);
  return (
    <Dialog.Root open onOpenChange={(o) => !o && close()}>
      <Dialog.Portal container={root}>
        <Dialog.Overlay ref={scrim} className="dialog-scrim enter">
          <Dialog.Content className="dialog" role="alertdialog" aria-describedby={undefined}>
            <Dialog.Title asChild>
              <h3>{title}</h3>
            </Dialog.Title>
            {children}
          </Dialog.Content>
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function ConfirmDialog({ title, body, detail, ok, cancel = "取消", danger, close }: Confirm & { close(ok: boolean): void }) {
  return (
    <DialogFrame title={title} close={() => close(false)}>
      <p>{body}</p>
      {detail !== undefined && <pre className="dialog-detail">{detail || "（空）"}</pre>}
      <div className="dialog-actions">
        <button className="text-btn" onClick={() => close(false)}>
          {cancel}
        </button>
        <button className={`text-btn${danger ? " danger-text" : ""}`} onClick={() => close(true)}>
          {ok}
        </button>
      </div>
    </DialogFrame>
  );
}

function PromptDialog({ title, label, value = "", placeholder, ok, check, close }: Prompt & { close(value: string | null): void }) {
  const [text, setText] = useState(value);
  const [error, setError] = useState("");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const c = check(text);
    if (c.ok) close(c.value);
    else setError(c.error);
  };
  return (
    <DialogFrame title={title} close={() => close(null)}>
      <form onSubmit={submit}>
        <label className={`field${error ? " error" : ""}`}>
          {label}
          <input autoFocus value={text} placeholder={placeholder} onChange={(e) => (setText(e.target.value), setError(""))} />
          {error && <span className="hint error-text">{error}</span>}
        </label>
        <div className="dialog-actions">
          <button type="button" className="text-btn" onClick={() => close(null)}>
            取消
          </button>
          <button className="text-btn">{ok}</button>
        </div>
      </form>
    </DialogFrame>
  );
}

/** How many panels are shown. */
let modals = 0;

/** A panel for choosing or settings: a bottom sheet on phones, a centred card on desktops. Back goes
 * to `onBack` when given, such as the page the panel came from, otherwise closes it. `focus` is what
 * takes the focus as it opens, rather than its first button: also when a menu opened the panel, where
 * a field's `autoFocus` loses it. */
export function Modal({
  title,
  onClose,
  onBack,
  focus,
  children,
}: {
  title: string;
  onClose(): void;
  onBack?: () => void;
  focus?: RefObject<HTMLElement | null>;
  children: ReactNode;
}) {
  const root = useContext(RootContext);
  const scrim = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  // Shown in place of another, as one page of the settings for the next, the panel stays put and its
  // content moves on, or back to the first page.
  const [replacing] = useState(() => modals > 0);
  useLayoutEffect(() => {
    modals++;
    return () => void modals--;
  }, []);
  useLeave([scrim, panel], () => modals > 0);
  useBack("dialog", onBack ?? onClose);
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal container={root}>
        <Dialog.Overlay ref={scrim} className={`modal-scrim${replacing ? "" : " enter"}`} />
        <Dialog.Content
          ref={panel}
          className={`modal ${replacing ? (onBack ? "forth" : "back") : "enter"}`}
          aria-describedby={undefined}
          // Without scrolling: the panel is still sliding in, and the app would scroll to where it is now.
          onOpenAutoFocus={focus && ((e) => (e.preventDefault(), focus.current!.focus({ preventScroll: true })))}
        >
          <div className="modal-head">
            <Dialog.Title asChild>
              <h3>{title}</h3>
            </Dialog.Title>
            <Tip label="关闭">
              <Dialog.Close className="icon-btn">
                <Icon of={X} />
              </Dialog.Close>
            </Tip>
          </div>
          <div className="modal-body">{children}</div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** Says what a button without words does, in a bubble on hovering it with a mouse; also its name for
 * screen readers. */
export function Tip({ label, children }: { label: string; children: ReactElement }) {
  const root = useContext(RootContext);
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        asChild
        aria-label={label}
        // Focused from the keyboard, it shows too; not as a panel opens and focuses its first button.
        onFocus={(e) => e.currentTarget.matches(":focus-visible") || e.preventDefault()}
      >
        {children}
      </Tooltip.Trigger>
      <Tooltip.Portal container={root}>
        <Tooltip.Content className="tip" sideOffset={6}>
          {label}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
