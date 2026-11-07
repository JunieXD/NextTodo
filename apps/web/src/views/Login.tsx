import { ChevronRight, Cloud, MonitorSmartphone } from "lucide-react";
import { type ReactNode, useState } from "react";
import { ApiError, api, deviceName, keepSession, native, serverUrl, setServer } from "../api/http";
import type { Session as SignedIn } from "../api/types/Session";
import type { User } from "../api/types/User";
import * as local from "../local";
import type { Session } from "../session";
import { Seg } from "../ui/controls";
import { useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";
import { Icon } from "../ui/icons";

/** `onSignIn` signs in with `request`, which asks the server and says who signed in; `onLocal` hears
 * of the session opened in local mode. */
export function Login({
  onSignIn,
  onLocal,
  notice,
}: {
  onSignIn(request: () => Promise<User>): Promise<void>;
  onLocal(session: Session): void;
  notice?: string;
}) {
  const invite = new URLSearchParams(location.search).get("invite") ?? "";
  const [mode, setMode] = useState<"login" | "register">(invite ? "register" : "login");
  // The app opened for the first time asks how it is to be used; the form is for a server.
  const [choosing, setChoosing] = useState(native && !serverUrl() && !invite && !notice);
  const [form, setForm] = useState({ server: serverUrl(), invite, username: "", display_name: "", password: "" });
  const [error, setError] = useState<{ message: string; field?: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const { confirm } = useFeedback();
  const set = (key: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement>) => {
    setForm({ ...form, [key]: e.target.value });
    setError(null);
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const { server, ...fields } = form;
      if (native) setServer(server);
      const body =
        mode === "login"
          ? { username: fields.username.trim(), password: fields.password, device: deviceName() }
          : { ...fields, invite: fields.invite.trim(), username: fields.username.trim(), display_name: fields.display_name.trim(), device: deviceName() };
      await onSignIn(async () => {
        const session = await api<SignedIn>("POST", mode === "login" ? "/auth/login" : "/auth/register", body);
        // First: on Android, leaving local mode takes the session there with it.
        await local.forgetMode();
        await keepSession(session.secret);
        if (invite) history.replaceState(null, "", "/");
        return session.user;
      });
    } catch (e) {
      // Saving on this device can fail too, say in a private window.
      setError(e instanceof ApiError ? { message: e.message, field: e.field } : { message: `登录失败：${e}` });
      setBusy(false);
    }
  };

  const enterLocal = async () => {
    setBusy(true);
    setError(null);
    try {
      const session = await local.enter((unsent) =>
        confirm({ title: "在本机使用？", body: `这台设备上有 ${unsent} 项改动还没发到服务器，在本机使用后会丢失。`, ok: "仍然使用", danger: true }),
      );
      if (session) return onLocal(session);
    } catch (e) {
      setError({ message: e instanceof Error ? e.message : String(e) });
    }
    setBusy(false);
  };

  const field = (key: keyof typeof form, label: string, props: React.InputHTMLAttributes<HTMLInputElement>, hint?: ReactNode) => (
    <label className={`field${error?.field === key ? " error" : ""}`}>
      <span>
        {label} {hint && <span className="hint">{hint}</span>}
      </span>
      <input value={form[key]} onChange={set(key)} required {...props} />
    </label>
  );
  if (choosing)
    return (
      <div className="login" data-tauri-drag-region="deep">
        <div className="login-card">
          <Brand />
          <Way icon={MonitorSmartphone} title="在本机使用" sub="数据只存在这台设备上，无须服务器和账号" disabled={busy} onClick={enterLocal} />
          <Way icon={Cloud} title="连接服务器" sub="在多台设备之间同步，须先部署服务端" disabled={busy} onClick={() => setChoosing(false)} />
          {error && <p className="form-error">{error.message}</p>}
          <p className="help-line">
            <Help to="#modes">了解两种用法</Help>
          </p>
        </div>
      </div>
    );
  return (
    <div className="login" data-tauri-drag-region="deep">
      <form className="login-card" onSubmit={submit}>
        <Brand />
        <Seg
          items={[
            ["login", "登录"],
            ["register", "邀请码注册"],
          ]}
          value={mode}
          onChange={(m) => (setMode(m), setError(null))}
        />
        {notice && !error && <p className="form-error">{notice}</p>}
        {native &&
          field(
            "server",
            "服务器地址",
            { autoComplete: "url", autoCapitalize: "none", inputMode: "url", placeholder: "todo.example.com" },
            <Help to="server">如何部署</Help>,
          )}
        {mode === "register" && field("invite", "邀请码", { autoComplete: "off" })}
        {field("username", "用户名", { autoComplete: "username", autoCapitalize: "none" }, mode === "register" ? "字母、数字和下划线，3–32 位" : undefined)}
        {mode === "register" && field("display_name", "显示名", { autoComplete: "nickname" }, "向他人显示的名称")}
        {field("password", "密码", { type: "password", autoComplete: mode === "login" ? "current-password" : "new-password" }, mode === "register" ? "至少 8 位" : undefined)}
        {error && <p className="form-error">{error.message}</p>}
        <button className="filled-btn" disabled={busy}>
          {mode === "login" ? "登录" : "注册"}
        </button>
        {native && (
          <div className="login-local">
            <button type="button" className="text-btn" disabled={busy} onClick={enterLocal}>
              在本机使用
            </button>
            <p>数据只存在这台设备上，不同步</p>
          </div>
        )}
      </form>
    </div>
  );
}

function Way({ icon, title, sub, disabled, onClick }: { icon: typeof Cloud; title: string; sub: string; disabled: boolean; onClick(): void }) {
  return (
    <button type="button" className="way" disabled={disabled} onClick={onClick}>
      <Icon of={icon} size={24} />
      <span className="way-text">
        <b>{title}</b>
        <span>{sub}</span>
      </span>
      <Icon of={ChevronRight} size={18} />
    </button>
  );
}

function Brand() {
  return (
    <div className="login-brand">
      <img className="brand-mark" src="/icon.svg" alt="" />
      <h1>NextTodo</h1>
      <p>理清先后，专注当下</p>
    </div>
  );
}

/** Local mode did not start: why, and what can be done. */
export function StartFailedPage({ error, onRetry, onServer }: { error: local.StartFailed; onRetry(): Promise<void>; onServer(): Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState("");
  const { confirm } = useFeedback();
  const act = (f: () => Promise<void>) => () => {
    setBusy(true);
    setFailed("");
    void f()
      .catch((e) => setFailed(e instanceof Error ? e.message : String(e)))
      .finally(() => setBusy(false));
  };
  const restore = () => local.restore((body) => confirm({ title: "用备份替换这台设备上的全部数据？", body, ok: "恢复", danger: true }));
  return (
    <div className="login" data-tauri-drag-region="deep">
      <div className="login-card">
        <Brand />
        <p className="form-error">本机模式无法启动：{error.message}</p>
        {failed && <p className="form-error">{failed}</p>}
        {error.locked && (
          <button className="filled-btn" disabled={busy} onClick={act(onRetry)}>
            重试
          </button>
        )}
        <div className="login-local">
          <button className="text-btn" disabled={busy} onClick={act(restore)}>
            恢复备份
          </button>
          <button className="text-btn" disabled={busy} onClick={act(onServer)}>
            改用服务器
          </button>
        </div>
      </div>
    </div>
  );
}
