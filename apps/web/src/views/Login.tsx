import { CircleCheck } from "lucide-react";
import { useState } from "react";
import { ApiError, api, deviceName } from "../api/http";
import type { Session } from "../api/types/Session";
import type { User } from "../api/types/User";
import { Seg } from "../ui/controls";
import { Icon } from "../ui/icons";

export function Login({ onSignedIn, notice }: { onSignedIn(user: User): Promise<void>; notice?: string }) {
  const invite = new URLSearchParams(location.search).get("invite") ?? "";
  const [mode, setMode] = useState<"login" | "register">(invite ? "register" : "login");
  const [form, setForm] = useState({ invite, username: "", display_name: "", password: "" });
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
      const body =
        mode === "login"
          ? { username: form.username.trim(), password: form.password, device: deviceName() }
          : { ...form, invite: form.invite.trim(), username: form.username.trim(), display_name: form.display_name.trim(), device: deviceName() };
      const session = await api<Session>("POST", mode === "login" ? "/auth/login" : "/auth/register", body);
      if (invite) history.replaceState(null, "", "/");
      await onSignedIn(session.user);
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      setError({ message: e.message, field: e.field });
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
