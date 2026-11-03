import { CircleCheck } from "lucide-react";
import { useState } from "react";
import { ApiError, api, deviceName, keepSession, native, serverUrl, setServer } from "../api/http";
import type { Session } from "../api/types/Session";
import type { User } from "../api/types/User";
import { Seg } from "../ui/controls";
import { Icon } from "../ui/icons";

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
      setError(e instanceof ApiError ? { message: e.message, field: e.field } : { message: `没能登录：${e}` });
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
    <div className="login">
      <form className="login-card" onSubmit={submit}>
        <div className="login-brand">
          <span className="brand-mark">
            <Icon of={CircleCheck} size={32} width={2} />
          </span>
          <h1>NextTodo</h1>
          <p>先做完前面的，再做后面的</p>
        </div>
        <Seg
          items={[
            ["login", "登录"],
            ["register", "用邀请码注册"],
          ]}
          value={mode}
          onChange={(m) => (setMode(m), setError(null))}
        />
        {notice && !error && <p className="form-error">{notice}</p>}
        {native && field("server", "服务器", { autoComplete: "url", autoCapitalize: "none", inputMode: "url" })}
        {mode === "register" && field("invite", "邀请码", { autoComplete: "off" })}
        {field("username", "用户名", { autoComplete: "username", autoCapitalize: "none" }, mode === "register" ? "字母、数字和下划线，3–32 位" : undefined)}
        {mode === "register" && field("display_name", "显示名", { autoComplete: "nickname" }, "别人看到的名字")}
        {field("password", "密码", { type: "password", autoComplete: mode === "login" ? "current-password" : "new-password" }, mode === "register" ? "8 位以上" : undefined)}
        {error && <p className="form-error">{error.message}</p>}
        <button className="filled-btn" disabled={busy}>
          {mode === "login" ? "登录" : "注册"}
        </button>
      </form>
    </div>
  );
}
