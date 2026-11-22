// The privacy lock's screen, and its page of the settings (docs/ui.md#隐私锁).

import { ChevronRight } from "lucide-react";
import { type ReactNode, useState } from "react";
import { ApiError, api } from "../api/http";
import { inLocalMode } from "../local";
import {
  IDLE,
  type Idle,
  MAX_TRIES,
  type PrivacyLock,
  canLock,
  isCode,
  lockNow,
  newCode,
  setCode,
  setIdle,
  setPin,
  tryPin,
  turnOff,
  turnOn,
  unlock,
  usePrivacyLock,
  useWrongTries,
} from "../lock";
import { Copyable, Seg, Switch } from "../ui/controls";
import { Modal, forUser, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";

/** Takes the place of the app, in every window, while it is locked. */
export function LockScreen({ lock }: { lock: PrivacyLock }) {
  const tries = useWrongTries();
  const [step, setStep] = useState<"pin" | "recover" | "new">("pin");
  // Too many wrong PINs, here or in another tab.
  const at = step === "pin" && tries >= MAX_TRIES ? "recover" : step;
  return (
    <div className="login" data-tauri-drag-region="deep">
      <div className="login-card">
        <div className="login-brand">
          <img className="brand-mark" src="/icon.svg" alt="" />
          <h1>NextTodo 已锁定</h1>
          {at === "new" && <p>设一个新的 PIN</p>}
        </div>
        {at === "pin" && <PinEntry lock={lock} onForgot={() => setStep("recover")} />}
        {at === "recover" && <Recover lock={lock} onBack={tries < MAX_TRIES ? () => setStep("pin") : undefined} onRecovered={() => setStep("new")} />}
        {at === "new" && (
          <NewPin onDone={(pin) => setPin(lock, pin).then(unlock)}>
            <button className="filled-btn">下一步</button>
          </NewPin>
        )}
      </div>
    </div>
  );
}

const digits = (typed: string) => typed.replace(/\D/g, "");

function PinEntry({ lock, onForgot }: { lock: PrivacyLock; onForgot(): void }) {
  const tries = useWrongTries();
  const [pin, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [wrong, setWrong] = useState(false);
  const attempt = async (typed: string) => {
    setBusy(true);
    // Right, it unlocks, and the app takes this one's place.
    if (await tryPin(lock, typed)) return;
    setBusy(false);
    setTyped("");
    setWrong(true);
  };
  return (
    <form onSubmit={(e) => (e.preventDefault(), pin && !busy && void attempt(pin))}>
      <label className={`field${wrong ? " error" : ""}`}>
        PIN
        <input
          autoFocus
          type="password"
          inputMode="numeric"
          autoComplete="off"
          maxLength={6}
          readOnly={busy}
          value={pin}
          aria-invalid={wrong}
          onChange={(e) => {
            const typed = digits(e.target.value);
            setTyped(typed);
            setWrong(false);
            if (typed.length === lock.length) void attempt(typed);
          }}
        />
      </label>
      {wrong && (
        <p className="form-error" role="alert">
          PIN 错误，还可尝试 {MAX_TRIES - tries} 次
        </p>
      )}
      <div className="login-local">
        <button type="button" className="text-btn" onClick={onForgot}>
          忘记 PIN？
        </button>
      </div>
    </form>
  );
}

/** Unlocking with the recovery code, checked here; or with the account's password, which the server
 * checks, when signed in to one. And then a new PIN. */
function Recover({ lock, onBack, onRecovered }: { lock: PrivacyLock; onBack?: () => void; onRecovered(): void }) {
  const [by, setBy] = useState<"code" | "password">("code");
  const [typed, setTyped] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const withPassword = !inLocalMode();
  const what = by === "code" ? "恢复码" : "账号密码";
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!typed) return setError(`请填写${what}`);
    setBusy(true);
    try {
      if (by === "code") return (await isCode(lock, typed)) ? onRecovered() : setError("恢复码错误");
      await api("POST", "/auth/verify", { password: typed });
      onRecovered();
    } catch (e) {
      setError(e instanceof ApiError && e.status === 400 ? "密码错误" : forUser("核对密码", e));
    } finally {
      setBusy(false);
    }
  };
  const switchTo = (to: typeof by) => () => {
    setBy(to);
    setTyped("");
    setError("");
  };
  return (
    <form onSubmit={submit}>
      {!onBack && <p className="form-error">PIN 已连续错误 {MAX_TRIES} 次，请用恢复码{withPassword && "或账号密码"}解锁</p>}
      <label className={`field${error ? " error" : ""}`}>
        {what}
        <input
          key={by}
          autoFocus
          {...(by === "code" ? { autoComplete: "off", autoCapitalize: "characters", spellCheck: false } : { type: "password", autoComplete: "current-password" })}
          value={typed}
          aria-invalid={!!error}
          onChange={(e) => (setTyped(e.target.value), setError(""))}
        />
      </label>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <button className="filled-btn" disabled={busy}>
        解锁
      </button>
      <div className="login-local">
        {withPassword && (
          // Not while one is being checked: its answer is of the other.
          <button type="button" className="text-btn" disabled={busy} onClick={switchTo(by === "code" ? "password" : "code")}>
            {by === "code" ? "用账号密码解锁" : "用恢复码解锁"}
          </button>
        )}
        {onBack && (
          <button type="button" className="text-btn" onClick={onBack}>
            输入 PIN
          </button>
        )}
      </div>
    </form>
  );
}

/** Asks for a new PIN twice; `children` are the buttons, one of them submitting. `onDone` once both
 * are the same. */
function NewPin({ onDone, children }: { onDone(pin: string): Promise<void>; children: ReactNode }) {
  const [first, setFirst] = useState<string | null>(null);
  const [pin, setTyped] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const { snack } = useFeedback();
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (pin.length < 4) return setError("PIN 是 4 到 6 位数字");
    setTyped("");
    if (first === null) return setFirst(pin);
    if (pin !== first) {
      setFirst(null);
      return setError("两次输入的不一样，请重新输入");
    }
    setBusy(true);
    onDone(pin)
      .catch((e: unknown) => snack(forUser("设置 PIN", e)))
      .finally(() => setBusy(false));
  };
  return (
    <form onSubmit={submit}>
      <label className={`field${error ? " error" : ""}`}>
        {first === null ? "输入 4 到 6 位数字" : "再输一遍"}
        <input
          // A new field for the second time, which password managers take for a new one.
          key={first === null ? "first" : "again"}
          autoFocus
          type="password"
          inputMode="numeric"
          autoComplete="off"
          maxLength={6}
          value={pin}
          aria-invalid={!!error}
          onChange={(e) => (setTyped(digits(e.target.value)), setError(""))}
        />
      </label>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {children}
    </form>
  );
}

/** A new recovery code; `onSaved` puts it in place, once the user has kept it. */
function NewCode({ code, onSaved }: { code: string; onSaved(): Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const { snack } = useFeedback();
  const saved = () => {
    setBusy(true);
    onSaved()
      .catch((e: unknown) => snack(forUser("保存恢复码", e)))
      .finally(() => setBusy(false));
  };
  return (
    <>
      <Copyable label="恢复码" text={code} />
      <p className="group-note">忘记 PIN 时用它解锁。仅显示这一次，请存入密码管理器或另行记录</p>
      <div className="actions">
        <button className="filled-btn" disabled={busy} onClick={saved}>
          我已存好
        </button>
      </div>
    </>
  );
}

const IDLE_ITEMS = IDLE.map((m): [Idle, string] => [m, `${m} 分钟`]);

export function LockPage() {
  const lock = usePrivacyLock();
  const [open, setOpen] = useState<"on" | "pin" | "code" | null>(null);
  const close = () => setOpen(null);
  if (!canLock()) return <p className="group-note">经 HTTP 打开时无法使用隐私锁，请改用 HTTPS</p>;
  return (
    <>
      <div className="group">
        <Switch label="隐私锁" sub="闲置一段时间后锁定 NextTodo，用 PIN 解锁。仅防他人查看，不加密数据" on={lock !== null} onChange={(on) => (on ? setOpen("on") : turnOff())} />
      </div>
      {lock && (
        <>
          <div className="group">
            <div className="row">
              <div className="row-inner controls">
                闲置多久锁定
                <span className="row-value">
                  <Seg<Idle> small label="闲置多久锁定" items={IDLE_ITEMS} value={lock.idle} onChange={(idle) => setIdle(lock, idle)} />
                </span>
              </div>
            </div>
            <Entry onClick={() => setOpen("pin")}>修改 PIN</Entry>
            <Entry onClick={() => setOpen("code")}>换一个恢复码</Entry>
          </div>
          <div className="group">
            <button className="row" onClick={lockNow}>
              <div className="row-inner">立即锁定</div>
            </button>
          </div>
        </>
      )}
      {open === "on" && <TurnOn onClose={close} />}
      {open === "pin" && lock && (
        <Modal title="修改 PIN" onClose={close}>
          <NewPin onDone={(pin) => setPin(lock, pin).then(close)}>
            <Actions onCancel={close} />
          </NewPin>
        </Modal>
      )}
      {open === "code" && lock && <ChangeCode lock={lock} onClose={close} />}
    </>
  );
}

/** A row that opens a dialog. */
function Entry({ onClick, children }: { onClick(): void; children: ReactNode }) {
  return (
    <button className="row" onClick={onClick}>
      <div className="row-inner">
        <span>{children}</span>
        <span className="chev" style={{ marginLeft: "auto" }}>
          <Icon of={ChevronRight} size={18} />
        </span>
      </div>
    </button>
  );
}

function Actions({ onCancel }: { onCancel(): void }) {
  return (
    <div className="actions">
      <button type="button" className="text-btn" onClick={onCancel}>
        取消
      </button>
      <button className="filled-btn">下一步</button>
    </div>
  );
}

function TurnOn({ onClose }: { onClose(): void }) {
  const [pin, setTyped] = useState<string | null>(null);
  const [code] = useState(newCode);
  return (
    <Modal title="设置 PIN" onClose={onClose}>
      {pin === null ? (
        <NewPin onDone={async (p) => setTyped(p)}>
          <Actions onCancel={onClose} />
        </NewPin>
      ) : (
        <NewCode code={code} onSaved={() => turnOn(pin, code).then(onClose)} />
      )}
    </Modal>
  );
}

function ChangeCode({ lock, onClose }: { lock: PrivacyLock; onClose(): void }) {
  const [code] = useState(newCode);
  return (
    <Modal title="换一个恢复码" onClose={onClose}>
      <p className="group-note">点“我已存好”后，旧的恢复码随即失效</p>
      <NewCode code={code} onSaved={() => setCode(lock, code).then(onClose)} />
    </Modal>
  );
}
