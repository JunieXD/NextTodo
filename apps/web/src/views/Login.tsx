import { useState } from "react";
import { ApiError, api, deviceName, keepSession, native, serverUrl, setServer } from "../api/http";
import type { Session } from "../api/types/Session";
import type { User } from "../api/types/User";
import { Seg } from "../ui/controls";

/** `onSignIn` signs in with `request`, which asks the server and says who signed in. */
export function Login({ onSignIn, notice }: { onSignIn(request: () => Promise<User>): Promise<void>; notice?: string }) {
  const invite = new URLSearchParams(location.search).get("invite") ?? "";
  const [mode, setMode] = useState<"login" | "register">(invite ? "register" : "login");
  const [form, setForm] = useState({ server: serverUrl(), invite, username: "", display_name: "", password: "" });
  const [error, setError] = useState<{ message: string; field?: string } | null>(null);
  const [busy, setBusy] = useState(false);
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
        const session = await api<Session>("POST", mode === "login" ? "/auth/login" : "/auth/register", body);
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

  const field = (key: keyof typeof form, label: string, props: React.InputHTMLAttributes<HTMLInputElement>, hint?: string) => (
    <label className={`field${error?.field === key ? " error" : ""}`}>
      <span>
        {label} {hint && <span className="hint">{hint}</span>}
      </span>
      <input value={form[key]} onChange={set(key)} required {...props} />
    </label>
  );
  return (
    <div className="login" data-tauri-drag-region="deep">
      <form className="login-card" onSubmit={submit}>
        <div className="login-brand">
          <img className="brand-mark" src="/icon.svg" alt="" />
          <h1>NextTodo</h1>
          <p>理清先后，专注当下</p>
        </div>
        <Seg
          items={[
            ["login", "登录"],
            ["register", "邀请码注册"],
          ]}
          value={mode}
          onChange={(m) => (setMode(m), setError(null))}
        />
        {notice && !error && <p className="form-error">{notice}</p>}
        {native && field("server", "服务器地址", { autoComplete: "url", autoCapitalize: "none", inputMode: "url", placeholder: "todo.example.com" })}
        {mode === "register" && field("invite", "邀请码", { autoComplete: "off" })}
        {field("username", "用户名", { autoComplete: "username", autoCapitalize: "none" }, mode === "register" ? "字母、数字和下划线，3–32 位" : undefined)}
        {mode === "register" && field("display_name", "显示名", { autoComplete: "nickname" }, "向他人显示的名称")}
        {field("password", "密码", { type: "password", autoComplete: mode === "login" ? "current-password" : "new-password" }, mode === "register" ? "至少 8 位" : undefined)}
        {error && <p className="form-error">{error.message}</p>}
        <button className="filled-btn" disabled={busy}>
          {mode === "login" ? "登录" : "注册"}
        </button>
      </form>
    </div>
  );
}
