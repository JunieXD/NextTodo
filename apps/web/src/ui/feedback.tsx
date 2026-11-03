// Snackbar, confirmation and input dialogs, and modal panels, all inside the app's root element so
// that the phone and desktop styles apply to them.

import { Dialog } from "radix-ui";
import { X } from "lucide-react";
import { type ReactNode, createContext, useContext, useMemo, useRef, useState } from "react";
import type { Checked } from "../domain/text";
import { useBack } from "./back";
import { Icon } from "./icons";

export const RootContext = createContext<HTMLElement | null>(null);

interface Snack {
  text: string;
  action?: string;
  onAction?: () => void;
  ms?: number;
}

interface Confirm {
  title: string;
  body: string;
  ok: string;
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

export function FeedbackProvider({ children }: { children: ReactNode }) {
  const [snack, setSnack] = useState<(Snack & { key: number }) | null>(null);
  const [open, setOpen] = useState<Open | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const feedback = useMemo<Feedback>(
    () => ({
      snack(s) {
        const shown = typeof s === "string" ? { text: s } : s;
        clearTimeout(timer.current);
        setSnack({ ...shown, key: Date.now() });
        timer.current = setTimeout(() => setSnack(null), shown.ms ?? 5000);
      },
      confirm: (c) => new Promise((resolve) => setOpen({ ...c, kind: "confirm", resolve })),
      prompt: (p) => new Promise((resolve) => setOpen({ ...p, kind: "prompt", resolve })),
    }),
    [],
  );
  const act = () => {
    setSnack(null);
    snack?.onAction?.();
  };
  return (
    <FeedbackContext value={feedback}>
      {children}
      {snack && (
        <div key={snack.key} className="snackbar enter" role="status">
          <span>{snack.text}</span>
          {snack.action && <button onClick={act}>{snack.action}</button>}
        </div>
      )}
      {open?.kind === "confirm" && <ConfirmDialog {...open} close={(ok) => (setOpen(null), open.resolve(ok))} />}
      {open?.kind === "prompt" && <PromptDialog {...open} close={(value) => (setOpen(null), open.resolve(value))} />}
    </FeedbackContext>
  );
}

function DialogFrame({ title, close, children }: { title: string; close(): void; children: ReactNode }) {
  const root = useContext(RootContext);
  useBack("dialog", close);
  return (
    <Dialog.Root open onOpenChange={(o) => !o && close()}>
      <Dialog.Portal container={root}>
        <Dialog.Overlay className="dialog-scrim enter">
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

function ConfirmDialog({ title, body, ok, danger, close }: Confirm & { close(ok: boolean): void }) {
  return (
    <DialogFrame title={title} close={() => close(false)}>
      <p>{body}</p>
      <div className="dialog-actions">
        <button className="text-btn" onClick={() => close(false)}>
          取消
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

/** A panel for choosing or settings: a bottom sheet on phones, a centred card on desktops. Back goes
 * to `onBack` when given, such as the page the panel came from, otherwise closes it. */
export function Modal({ title, onClose, onBack, children }: { title: string; onClose(): void; onBack?: () => void; children: ReactNode }) {
  const root = useContext(RootContext);
  useBack("dialog", onBack ?? onClose);
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal container={root}>
        <Dialog.Overlay className="modal-scrim enter" />
        <Dialog.Content className="modal enter" aria-describedby={undefined}>
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
