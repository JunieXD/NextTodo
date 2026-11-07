// Snackbar, confirmation and input dialogs, and modal panels, all inside the app's root element so
// that the phone and desktop styles apply to them.

import { Dialog } from "radix-ui";
import { X } from "lucide-react";
import { type ReactNode, createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const feedback = useMemo<Feedback>(
    () => ({
      snack(s) {
        const shown = typeof s === "string" ? { text: s } : s;
        clearTimeout(timer.current);
        setSnack({ ...shown, key: Date.now() });
        timer.current = setTimeout(() => setSnack(null), shown.ms ?? 5000);
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
  const act = () => {
    setSnack(null);
    snack?.onAction?.();
  };
  return (
    <FeedbackContext value={feedback}>
      {children}
      {snack && <Snackbar key={snack.key} text={snack.text} action={snack.action} onAction={act} />}
      {open?.kind === "confirm" && <ConfirmDialog {...open} close={(ok) => (setOpen(null), open.resolve(ok))} />}
      {open?.kind === "prompt" && <PromptDialog {...open} close={(value) => (setOpen(null), open.resolve(value))} />}
    </FeedbackContext>
  );
}

/** A new one comes in over the one it replaces as that fades. */
function Snackbar({ text, action, onAction }: { text: string; action?: string; onAction(): void }) {
  const el = useRef<HTMLDivElement>(null);
  useLeave([el]);
  return (
    <div ref={el} className="snackbar enter" role="status">
      <span>{text}</span>
      {action && <button onClick={onAction}>{action}</button>}
    </div>
  );
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
 * to `onBack` when given, such as the page the panel came from, otherwise closes it. */
export function Modal({ title, onClose, onBack, children }: { title: string; onClose(): void; onBack?: () => void; children: ReactNode }) {
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
        <Dialog.Content ref={panel} className={`modal ${replacing ? (onBack ? "forth" : "back") : "enter"}`} aria-describedby={undefined}>
          <div className="modal-head">
            <Dialog.Title asChild>
              <h3>{title}</h3>
            </Dialog.Title>
            <Dialog.Close className="icon-btn" aria-label="关闭">
              <Icon of={X} />
            </Dialog.Close>
          </div>
          <div className="modal-body">{children}</div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
