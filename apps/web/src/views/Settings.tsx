// Account, devices and tokens, invites, appearance, versions.

import { ChevronRight, Copy, KeyRound, Mail, Monitor, Plus, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import { ApiError, api, app, serverUrl } from "../api/http";
import type { CreatedToken } from "../api/types/CreatedToken";
import type { Invite } from "../api/types/Invite";
import type { Scope } from "../api/types/Scope";
import type { TokenInfo } from "../api/types/TokenInfo";
import type { User } from "../api/types/User";
import { ago } from "../domain/dates";
import { checkDisplayName, checkTokenName } from "../domain/text";
import { useSession } from "../session";
import { type Theme, setTheme, useTheme } from "../theme";
import { Seg } from "../ui/controls";
import { Modal, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { About } from "./About";
import { AiPage, useAiSummary } from "./AiSettings";
import type { SettingsPage } from "./nav";
import { BackgroundSettings } from "./Background";
import { StartAtLogin } from "./StartAtLogin";

/** The link that opens registration with an invite code: on the server, which in the app is not
 * where the page comes from. */
const inviteLink = (code: string) => `${serverUrl() || location.origin}/?invite=${code}`;

type Page = SettingsPage;

export function Settings({ start = "main", onClose, onSignOut }: { start?: Page; onClose(): void; onSignOut(): void }) {
  const session = useSession();
  const [user, setUser] = useState(session.user);
  const [page, setPage] = useState<Page>(start);
  const [tokens, setTokens] = useState<TokenInfo[] | null>(null);
  const [invites, setInvites] = useState<Invite[] | null>(null);
  const [error, setError] = useState("");
  const { prompt, confirm, snack } = useFeedback();
  const theme = useTheme();

  const failed = (e: unknown) => {
    if (!(e instanceof ApiError)) throw e;
    setError(e.message);
  };
  const loadTokens = () => api<TokenInfo[]>("GET", "/tokens").then(setTokens, failed);
  useEffect(() => {
    loadTokens();
    if (user.is_admin) api<Invite[]>("GET", "/invites").then(setInvites, failed);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- loads once when opened
  }, []);

  const rename = async () => {
    const name = await prompt({ title: "修改显示名", label: "显示名", value: user.display_name, ok: "保存", check: checkDisplayName });
    if (name === null) return;
    try {
      const updated = await api<User>("PATCH", "/me", { display_name: name });
      session.setUser(updated);
      setUser(updated);
    } catch (e) {
      failed(e);
    }
  };
  const revoke = async (t: TokenInfo) => {
    const what = t.kind === "session" ? "退出该设备" : `删除令牌“${t.name}”`;
    const body = t.kind === "session" ? `“${t.name}”将需要重新登录。` : "使用此令牌的 agent 和命令行将立即失去访问权限。";
    if (!(await confirm({ title: `${what}？`, body, ok: t.kind === "session" ? "退出" : "删除", danger: true }))) return;
    await api("DELETE", `/tokens/${t.id}`).then(loadTokens, failed);
  };
  const invite = async () => {
    try {
      const made = await api<Invite>("POST", "/invites");
      setInvites((list) => [made, ...(list ?? [])]);
      await copy(inviteLink(made.code));
      snack("已复制邀请链接");
    } catch (e) {
      failed(e);
    }
  };

  if (page === "password") return <PasswordPage onDone={() => setPage("main")} onClose={onClose} />;
  if (page === "ai") return <AiPage onBack={() => setPage("main")} onClose={onClose} />;
  if (page === "token") {
    return <TokenPage onCreated={(c) => (loadTokens(), setPage({ secret: c.secret, name: c.token.name }))} onBack={() => setPage("main")} onClose={onClose} />;
  }
  if (typeof page === "object") {
    return (
      <Modal title="令牌已生成" onClose={onClose} onBack={() => setPage("main")}>
        <p className="empty">以下是“{page.name}”的令牌，仅显示一次。可在命令行中用 nexttodo login 登录，或填入 agent 的 MCP 配置。</p>
        <div className="secret">
          <span>{page.secret}</span>
          <button className="icon-btn sm" aria-label="复制令牌" onClick={() => copy(page.secret).then(() => snack("已复制令牌"))}>
            <Icon of={Copy} size={18} />
          </button>
        </div>
        <div className="detail-actions flex justify-end">
          <button className="filled-btn" onClick={() => setPage("main")}>
            完成
          </button>
        </div>
      </Modal>
    );
  }

  const sessions = tokens?.filter((t) => t.kind === "session") ?? [];
  const apiTokens = tokens?.filter((t) => t.kind === "api") ?? [];
  const now = new Date();
  const used = (t: TokenInfo) => (t.last_used_at ? `${ago(t.last_used_at, now)}使用` : "未使用");
  return (
    <Modal title="设置" onClose={onClose}>
      {error && <p className="form-error">{error}</p>}
      <div className="group">
        <button className="row" onClick={rename}>
          <div className="row-inner">
            <span className="avatar" style={{ margin: 0 }}>
              {[...user.display_name][0]}
            </span>
            <span className="folder-main">
              <span>{user.display_name}</span>
              <span className="folder-sub">用户名 {user.username}</span>
            </span>
            <span className="chev">
              <Icon of={ChevronRight} size={18} />
            </span>
          </div>
        </button>
        <button className="row" onClick={() => setPage("password")}>
          <div className="row-inner">
            <span>修改密码</span>
            <span className="chev" style={{ marginLeft: "auto" }}>
              <Icon of={ChevronRight} size={18} />
            </span>
          </div>
        </button>
      </div>

      <h2 className="group-title">AI 整理</h2>
      <div className="group">
        <AiRow onOpen={() => setPage("ai")} />
      </div>

      <h2 className="group-title">
        <span>令牌</span>
        <span>供 agent 和命令行使用</span>
      </h2>
      <div className="group">
        {apiTokens.map((t) => (
          <TokenRow key={t.id} icon={KeyRound} title={t.name} sub={`${t.scope === "write" ? "读写" : "只读"}，${used(t)}`} action={["删除", () => revoke(t)]} />
        ))}
        <button className="row add-row" onClick={() => setPage("token")}>
          <div className="row-inner">
            <Icon of={Plus} />
            生成令牌
          </div>
        </button>
      </div>

      <h2 className="group-title">已登录的设备</h2>
      <div className="group">
        {sessions.map((t) => (
          <TokenRow key={t.id} icon={Monitor} title={t.name} sub={t.current ? "本机" : used(t)} action={t.current ? undefined : ["退出", () => revoke(t)]} />
        ))}
      </div>

      {invites && (
        <>
          <h2 className="group-title">
            <span>邀请注册</span>
            <span>每个邀请码仅可注册一个账号</span>
          </h2>
          <div className="group">
            {invites.map((i) => (
              <div key={i.code} className="row">
                <div className="row-inner">
                  <span className="token-ic">
                    <Icon of={Mail} size={18} />
                  </span>
                  <span className="folder-main">
                    <span className="tabular-nums">{i.code}</span>
                    <span className="folder-sub">{i.used_by ? `${i.used_by} 已注册` : `未使用，${ago(i.created_at, now)}生成`}</span>
                  </span>
                  {!i.used_by && (
                    <button className="icon-btn sm" aria-label="复制邀请链接" onClick={() => copy(inviteLink(i.code)).then(() => snack("已复制邀请链接"))}>
                      <Icon of={Copy} size={16} />
                    </button>
                  )}
                </div>
              </div>
            ))}
            <button className="row add-row" onClick={invite}>
              <div className="row-inner">
                <Icon of={Mail} />
                生成邀请码
              </div>
            </button>
          </div>
        </>
      )}

      {app === "android" && <BackgroundSettings />}
      {app === "desktop" && <StartAtLogin />}

      <h2 className="group-title">外观</h2>
      <div style={{ margin: "0 16px" }}>
        <Seg<Theme>
          items={[
            ["system", "跟随系统"],
            ["light", "浅色"],
            ["dark", "深色"],
          ]}
          value={theme}
          onChange={setTheme}
        />
      </div>
      <About />
      <div className="group danger" style={{ marginTop: 24 }}>
        <button className="row" onClick={onSignOut}>
          <div className="row-inner">退出登录</div>
        </button>
      </div>
    </Modal>
  );
}

function AiRow({ onOpen }: { onOpen(): void }) {
  const summary = useAiSummary();
  return (
    <button className="row" onClick={onOpen}>
      <div className="row-inner">
        <span className="token-ic">
          <Icon of={Sparkles} size={18} />
        </span>
        <span>模型</span>
        <span className="row-value">{summary}</span>
        <span className="chev">
          <Icon of={ChevronRight} size={18} />
        </span>
      </div>
    </button>
  );
}

function TokenRow({ icon, title, sub, action }: { icon: typeof KeyRound; title: string; sub: string; action?: [string, () => void] }) {
  return (
    <div className="row">
      <div className="row-inner">
        <span className="token-ic">
          <Icon of={icon} size={18} />
        </span>
        <span className="folder-main">
          <span>{title}</span>
          <span className="folder-sub">{sub}</span>
        </span>
        {action && (
          <button className="text-btn danger-text" onClick={action[1]}>
            {action[0]}
          </button>
        )}
      </div>
    </div>
  );
}

function PasswordPage({ onDone, onClose }: { onDone(): void; onClose(): void }) {
  const [old, setOld] = useState("");
  const [fresh, setFresh] = useState("");
  const [error, setError] = useState("");
  const { snack } = useFeedback();
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      await api("POST", "/auth/password", { old_password: old, new_password: fresh });
      snack("密码已修改，其他设备需重新登录");
      onDone();
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      setError(e.message);
    }
  };
  return (
    <Modal title="修改密码" onClose={onClose} onBack={onDone}>
      <form onSubmit={submit}>
        <label className="field">
          当前密码
          <input type="password" autoComplete="current-password" value={old} onChange={(e) => setOld(e.target.value)} required />
        </label>
        <label className="field">
          <span>
            新密码 <span className="hint">至少 8 位</span>
          </span>
          <input type="password" autoComplete="new-password" value={fresh} onChange={(e) => setFresh(e.target.value)} required minLength={8} />
        </label>
        {error && <p className="form-error">{error}</p>}
        <div className="detail-actions flex justify-end gap-2">
          <button type="button" className="text-btn" onClick={onDone}>
            取消
          </button>
          <button className="filled-btn">修改</button>
        </div>
      </form>
    </Modal>
  );
}

function TokenPage({ onCreated, onBack, onClose }: { onCreated(c: CreatedToken): void; onBack(): void; onClose(): void }) {
  const [name, setName] = useState("");
  const [scope, setScope] = useState<Scope>("write");
  const [error, setError] = useState("");
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const c = checkTokenName(name);
    if (!c.ok) return setError(c.error);
    try {
      onCreated(await api<CreatedToken>("POST", "/tokens", { name: c.value, scope }));
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      setError(e.message);
    }
  };
  return (
    <Modal title="生成令牌" onClose={onClose} onBack={onBack}>
      <form onSubmit={submit}>
        <label className="field">
          <span>
            名称 <span className="hint">显示在它所做的改动旁，如 Claude Code</span>
          </span>
          <input autoFocus value={name} onChange={(e) => (setName(e.target.value), setError(""))} />
        </label>
        <div style={{ margin: "0 16px 14px" }}>
          <Seg<Scope>
            items={[
              ["write", "读写"],
              ["read", "只读"],
            ]}
            value={scope}
            onChange={setScope}
          />
        </div>
        {error && <p className="form-error">{error}</p>}
        <div className="detail-actions flex justify-end gap-2">
          <button type="button" className="text-btn" onClick={onBack}>
            取消
          </button>
          <button className="filled-btn">生成</button>
        </div>
      </form>
    </Modal>
  );
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text);
}
