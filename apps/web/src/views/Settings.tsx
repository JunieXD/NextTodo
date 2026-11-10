// The settings: a home page that lists the sections, each a page of its own (docs/ui.md#设置).

import { Bell, ChevronRight, Copy, HardDrive, Info, KeyRound, type LucideIcon, Mail, Monitor, Palette, Plus, Sparkles, SquareTerminal } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { useLocation } from "wouter";
import { ApiError, api, app, desktopOs, serverUrl } from "../api/http";
import type { CreatedToken } from "../api/types/CreatedToken";
import type { Invite } from "../api/types/Invite";
import type { Scope } from "../api/types/Scope";
import type { TokenInfo } from "../api/types/TokenInfo";
import type { User } from "../api/types/User";
import { ago } from "../domain/dates";
import { type CliStatus, cli } from "../desktop";
import { checkDisplayName, checkTokenName } from "../domain/text";
import * as local from "../local";
import { newer, useServerVersions } from "../server-update";
import { useSession } from "../session";
import { type Theme, setTheme, useTheme } from "../theme";
import { Seg } from "../ui/controls";
import { Modal, Tip, forUser, useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";
import { Icon } from "../ui/icons";
import { usePending } from "../updates";
import { About } from "./About";
import { AiPage, useAiSummary } from "./AiSettings";
import { BackgroundSettings } from "./Background";
import { type SettingsSection, settingsUrl } from "./nav";
import { StartAtLogin } from "./StartAtLogin";

/** The link that opens registration with an invite code: on the server, which in the app is not
 * where the page comes from. */
const inviteLink = (code: string) => `${serverUrl() || location.origin}/?invite=${code}`;

const TITLES: Record<SettingsSection, string> = {
  account: "账号",
  local: "本机模式",
  appearance: "外观",
  background: "提醒和后台",
  ai: "AI 整理",
  agent: "agent 接入",
  invites: "邀请注册",
  about: "关于",
};

export const settingsTitle = (section: SettingsSection) => TITLES[section];

const ICONS: Record<Exclude<SettingsSection, "account">, LucideIcon> = {
  local: HardDrive,
  appearance: Palette,
  background: Bell,
  ai: Sparkles,
  agent: KeyRound,
  invites: Mail,
  about: Info,
};

const THEMES: [Theme, string][] = [
  ["system", "跟随系统"],
  ["light", "浅色"],
  ["dark", "深色"],
];

/** The sections here, in the home page's groups. Agents reach no phone in local mode: its server
 * listens on the phone alone, which has no command line. */
function useSections(): SettingsSection[][] {
  const { user } = useSession();
  const inLocal = local.inLocalMode();
  const groups: SettingsSection[][] = [[inLocal ? "local" : "account"], app ? ["appearance", "background"] : ["appearance"]];
  groups.push(!inLocal || app === "desktop" ? ["ai", "agent"] : ["ai"]);
  if (user.is_admin && !inLocal) groups.push(["invites"]);
  groups.push(["about"]);
  return groups;
}

/** The section at `location`: null on the home page, undefined for one not here, such as the
 * invites for someone not an admin. */
export function useSectionAt(location: string): SettingsSection | null | undefined {
  const sections = useSections().flat();
  return location === settingsUrl() ? null : sections.find((s) => location === settingsUrl(s));
}

export function SettingsHome() {
  const [, navigate] = useLocation();
  const { user } = useSession();
  const theme = useTheme();
  const ai = useAiSummary();
  const pending = usePending();
  const server = useServerVersions();
  const upgradable = user.is_admin && !local.inLocalMode() && server?.latest && newer(server.latest, server.server);
  const values: Partial<Record<SettingsSection, string>> = {
    appearance: THEMES.find(([t]) => t === theme)![1],
    ai,
    about: pending ? "有新版本" : upgradable ? "服务器可升级" : undefined,
  };
  const open = (s: SettingsSection) => () => navigate(settingsUrl(s));
  return (
    <div className="settings-page">
      {useSections().map((group) => (
        <div key={group[0]} className="group">
          {group.map((s) => {
            if (s === "account") {
              return (
                <Entry key={s} onClick={open(s)}>
                  <span className="avatar" style={{ margin: 0 }}>
                    {[...user.display_name][0]}
                  </span>
                  <span className="folder-main">
                    <span>{user.display_name}</span>
                    <span className="folder-sub">用户名 {user.username}</span>
                  </span>
                </Entry>
              );
            }
            return (
              <Entry key={s} onClick={open(s)}>
                <span className="token-ic">
                  <Icon of={ICONS[s]} size={18} />
                </span>
                {s === "local" ? (
                  <span className="folder-main">
                    <span>本机模式</span>
                    <span className="folder-sub">数据只存在这台设备上，不同步</span>
                  </span>
                ) : (
                  <>
                    <span>{TITLES[s]}</span>
                    <span className="row-value">{values[s]}</span>
                  </>
                )}
              </Entry>
            );
          })}
        </div>
      ))}
    </div>
  );
}

/** A row on the home page that leads to a section. */
function Entry({ onClick, children }: { onClick(): void; children: ReactNode }) {
  return (
    <button className="row" onClick={onClick}>
      <div className="row-inner">
        {children}
        <span className="chev">
          <Icon of={ChevronRight} size={18} />
        </span>
      </div>
    </button>
  );
}

export function SettingsPage({ section, onSignOut }: { section: SettingsSection; onSignOut(): void }) {
  const pages: Record<SettingsSection, () => ReactNode> = {
    account: () => <Account onSignOut={onSignOut} />,
    local: () => <LocalMode />,
    appearance: () => <Appearance />,
    background: () => (app === "android" ? <BackgroundSettings /> : <StartAtLogin />),
    ai: () => <AiPage />,
    agent: () => <Agent />,
    invites: () => <Invites />,
    about: () => <About />,
  };
  return <div className="settings-page">{pages[section]()}</div>;
}

/** What went wrong talking to the server, shown at the top of the page. */
function useFailure(): [string, (e: unknown) => void] {
  const [error, setError] = useState("");
  return [
    error,
    (e: unknown) => {
      if (!(e instanceof ApiError)) throw e;
      setError(e.message);
    },
  ];
}

/** The tokens of a kind, loaded once the page opens, and taking one away. */
function useTokens(kind: TokenInfo["kind"], failed: (e: unknown) => void) {
  const { confirm } = useFeedback();
  const [tokens, setTokens] = useState<TokenInfo[] | null>(null);
  const load = () => api<TokenInfo[]>("GET", "/tokens").then((all) => setTokens(all.filter((t) => t.kind === kind)), failed);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- loads once when opened
  }, []);
  const revoke = async (t: TokenInfo) => {
    const what = t.kind === "session" ? "退出该设备" : `删除令牌“${t.name}”`;
    const body = t.kind === "session" ? `“${t.name}”将需要重新登录。` : "使用此令牌的 agent 和命令行将立即失去访问权限。";
    if (!(await confirm({ title: `${what}？`, body, ok: t.kind === "session" ? "退出" : "删除", danger: true }))) return;
    await api("DELETE", `/tokens/${t.id}`).then(load, failed);
  };
  return { tokens, load, revoke };
}

const used = (t: TokenInfo, now: Date) => (t.last_used_at ? `${ago(t.last_used_at, now)}使用` : "未使用");

function Account({ onSignOut }: { onSignOut(): void }) {
  const session = useSession();
  const [user, setUser] = useState(session.user);
  const [error, failed] = useFailure();
  const { tokens: sessions, revoke } = useTokens("session", failed);
  const [password, setPassword] = useState(false);
  const { prompt } = useFeedback();
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
  const now = new Date();
  return (
    <>
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
        <button className="row" onClick={() => setPassword(true)}>
          <div className="row-inner">
            <span>修改密码</span>
            <span className="chev" style={{ marginLeft: "auto" }}>
              <Icon of={ChevronRight} size={18} />
            </span>
          </div>
        </button>
      </div>
      <h2 className="group-title">已登录的设备</h2>
      <div className="group">
        {sessions?.map((t) => (
          <TokenRow key={t.id} icon={Monitor} title={t.name} sub={t.current ? "本机" : used(t, now)} action={t.current ? undefined : ["退出", () => revoke(t)]} />
        ))}
      </div>
      <div className="group danger" style={{ marginTop: 24 }}>
        <button className="row" onClick={onSignOut}>
          <div className="row-inner">退出登录</div>
        </button>
      </div>
      {password && <PasswordDialog onClose={() => setPassword(false)} />}
    </>
  );
}

/** In place of the account: the data is on this device only, its backups, and how to move to a
 * server. */
function LocalMode() {
  const session = useSession();
  const { confirm, snack } = useFeedback();
  const backup = async () => {
    try {
      const to = await local.backup(session.store);
      if (to) snack(`已备份到 ${to}`);
    } catch (e) {
      snack(forUser("备份", e));
    }
  };
  const restore = () =>
    local
      .restore((body) => confirm({ title: "用备份替换这台设备上的全部数据？", body, ok: "恢复", danger: true }), session)
      .catch((e) => snack(forUser("恢复", e)));
  const leave = async () => {
    const body = "本机的数据会留在这台设备上，以后再选“在本机使用”就能接着用。NextTodo 会重新启动。";
    if (!(await confirm({ title: "改用服务器？", body, ok: "改用服务器" }))) return;
    await local.leave(session).catch((e) => snack(forUser("改用服务器", e)));
  };
  return (
    <>
      <p className="empty">
        数据只存在这台设备上，不同步。定期备份，换设备时用备份恢复。<Help to="local">说明</Help>
      </p>
      <div className="group">
        <Action label="备份" onClick={backup} />
        <Action label="恢复" onClick={restore} />
        <Action label="改用服务器" onClick={leave} />
      </div>
    </>
  );
}

function Action({ label, onClick }: { label: string; onClick(): void }) {
  return (
    <button className="row" onClick={onClick}>
      <div className="row-inner">
        <span>{label}</span>
        <span className="chev" style={{ marginLeft: "auto" }}>
          <Icon of={ChevronRight} size={18} />
        </span>
      </div>
    </button>
  );
}

function Appearance() {
  const theme = useTheme();
  return (
    <>
      <h2 className="group-title">主题</h2>
      <div style={{ margin: "0 16px" }}>
        <Seg<Theme> label="主题" items={THEMES} value={theme} onChange={setTheme} />
      </div>
    </>
  );
}

function Agent() {
  const [error, failed] = useFailure();
  const { tokens, load, revoke } = useTokens("api", failed);
  const [dialog, setDialog] = useState<"new" | { secret: string; name: string } | null>(null);
  const now = new Date();
  return (
    <>
      {error && <p className="form-error">{error}</p>}
      <h2 className="group-title">
        <span>令牌</span>
        <span>供 agent 和命令行使用</span>
      </h2>
      <div className="group">
        {tokens?.map((t) => (
          <TokenRow key={t.id} icon={KeyRound} title={t.name} sub={`${t.scope === "write" ? "读写" : "只读"}，${used(t, now)}`} action={["删除", () => revoke(t)]} />
        ))}
        <button className="row add-row" onClick={() => setDialog("new")}>
          <div className="row-inner">
            <Icon of={Plus} />
            生成令牌
          </div>
        </button>
      </div>
      {app === "desktop" && <CommandLine />}
      {dialog === "new" && <TokenPage onCreated={(c) => (void load(), setDialog({ secret: c.secret, name: c.token.name }))} onClose={() => setDialog(null)} />}
      {dialog && dialog !== "new" && <TokenCreated {...dialog} onDone={() => setDialog(null)} />}
    </>
  );
}

function Invites() {
  const [invites, setInvites] = useState<Invite[] | null>(null);
  const [error, failed] = useFailure();
  const { snack } = useFeedback();
  useEffect(() => {
    api<Invite[]>("GET", "/invites").then(setInvites, failed);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- loads once when opened
  }, []);
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
  const now = new Date();
  return (
    <>
      {error && <p className="form-error">{error}</p>}
      <p className="empty">每人一个账号，数据彼此独立。每个邀请码只能注册一个账号。</p>
      <div className="group">
        {invites?.map((i) => (
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
                <Tip label="复制邀请链接">
                  <button className="icon-btn sm" onClick={() => copy(inviteLink(i.code)).then(() => snack("已复制邀请链接"))}>
                    <Icon of={Copy} size={16} />
                  </button>
                </Tip>
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
  );
}

/** The command line the app carries, and putting it where a terminal finds it (docs/desktop.md#命令行). */
function CommandLine() {
  const { confirm, snack } = useFeedback();
  const [status, setStatus] = useState<CliStatus | null>(null);
  useEffect(() => {
    cli.status().then(setStatus, (e) => snack(forUser("读取命令行的状态", e)));
  }, [snack]);
  const install = async () => {
    const replace = status?.state === "taken";
    const body = `/usr/local/bin/nexttodo 现在${status?.state === "taken" ? status.by : ""}，将换成 NextTodo 带的命令行。`;
    if (replace && !(await confirm({ title: "替换已有的 nexttodo？", body, ok: "替换", danger: true }))) return;
    await cli.install(replace).then(
      () => (setStatus({ state: "installed" }), snack(desktopOs === "Windows" ? "已安装命令行，新打开的终端可以使用" : "已安装命令行")),
      (e) => snack(forUser("安装命令行", e)),
    );
  };
  const sub = { installed: "已安装，在终端里运行 nexttodo", missing: "未安装", taken: "已有另一个 nexttodo" };
  return (
    <>
      <h2 className="group-title">
        <span>命令行</span>
        <span>供 agent 在终端使用</span>
      </h2>
      <div className="group">
        <div className="row">
          <div className="row-inner">
            <span className="token-ic">
              <Icon of={SquareTerminal} size={18} />
            </span>
            <span className="folder-main">
              <span>nexttodo</span>
              <span className="folder-sub">{status && sub[status.state]}</span>
            </span>
            {status && status.state !== "installed" && (
              <button className="text-btn" onClick={install}>
                {status.state === "taken" ? "替换" : "安装"}
              </button>
            )}
          </div>
        </div>
      </div>
    </>
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

function PasswordDialog({ onClose }: { onClose(): void }) {
  const [old, setOld] = useState("");
  const [fresh, setFresh] = useState("");
  const [error, setError] = useState<{ message: string; field?: string } | null>(null);
  const { snack } = useFeedback();
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!old) return setError({ message: "请填写当前密码", field: "old_password" });
    if (!fresh) return setError({ message: "请填写新密码", field: "new_password" });
    try {
      await api("POST", "/auth/password", { old_password: old, new_password: fresh });
      snack("密码已修改，其他设备需重新登录");
      onClose();
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      setError({ message: e.message, field: e.field });
    }
  };
  const wrong = (field: string) => error?.field === field;
  return (
    <Modal title="修改密码" onClose={onClose}>
      <form onSubmit={submit}>
        <label className={`field${wrong("old_password") ? " error" : ""}`}>
          当前密码
          <input
            type="password"
            autoComplete="current-password"
            value={old}
            onChange={(e) => (setOld(e.target.value), setError(null))}
            aria-invalid={wrong("old_password")}
          />
        </label>
        <label className={`field${wrong("new_password") ? " error" : ""}`}>
          <span>
            新密码 <span className="hint">至少 8 位</span>
          </span>
          <input
            type="password"
            autoComplete="new-password"
            value={fresh}
            onChange={(e) => (setFresh(e.target.value), setError(null))}
            aria-invalid={wrong("new_password")}
          />
        </label>
        {error && (
          <p className="form-error" role="alert">
            {error.message}
          </p>
        )}
        <div className="actions">
          <button type="button" className="text-btn" onClick={onClose}>
            取消
          </button>
          <button className="filled-btn">修改</button>
        </div>
      </form>
    </Modal>
  );
}

function TokenPage({ onCreated, onClose }: { onCreated(c: CreatedToken): void; onClose(): void }) {
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
    <Modal title="生成令牌" onClose={onClose}>
      <form onSubmit={submit}>
        <label className="field">
          <span>
            名称 <span className="hint">显示在它所做的改动旁，如 Claude Code</span>
          </span>
          <input autoFocus value={name} onChange={(e) => (setName(e.target.value), setError(""))} />
        </label>
        <div style={{ margin: "0 16px 14px" }}>
          <Seg<Scope>
            label="权限"
            items={[
              ["write", "读写"],
              ["read", "只读"],
            ]}
            value={scope}
            onChange={setScope}
          />
        </div>
        {error && <p className="form-error">{error}</p>}
        <div className="actions">
          <button type="button" className="text-btn" onClick={onClose}>
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

/** A new token, shown this once, and the ways to connect an agent with it, filled in. In local mode
 * the app's address changes each time it starts: agents go through the command line, which finds
 * it (docs/local.md#agent-接入). */
/** In place of the form it was made with, as its next page. */
function TokenCreated({ name, secret, onDone }: { name: string; secret: string; onDone(): void }) {
  const inLocal = local.inLocalMode();
  const url = inLocal ? "local" : serverUrl() || location.origin;
  const viaCli = (agent: string) => `${agent} mcp add nexttodo --env NEXTTODO_URL=local --env NEXTTODO_TOKEN=${secret} -- nexttodo mcp`;
  return (
    <Modal title="令牌已生成" onClose={onDone} onBack={onDone}>
      <p className="empty">
        “{name}”的令牌只显示这一次。下面的写法已经填好{inLocal ? "令牌，都要先装命令行" : "服务器地址和令牌"}，复制即可。
        {app === "desktop" ? "命令行在本页下方的“命令行”一组安装。" : <Help to="agent#cli">如何安装命令行</Help>}
      </p>
      <Copyable label="令牌" text={secret} />
      {inLocal ? (
        <Copyable label="Claude Code" text={viaCli("claude")} />
      ) : (
        <Copyable label="Claude Code" text={`claude mcp add --transport http nexttodo ${url}/mcp --header "Authorization: Bearer ${secret}"`} />
      )}
      <EnvVars url={url} secret={secret} />
      {inLocal ? (
        <Copyable label="Codex" text={viaCli("codex")} />
      ) : (
        <Copyable label="Codex" hint="~/.codex/config.toml" text={`[mcp_servers.nexttodo]\nurl = "${url}/mcp"\nbearer_token_env_var = "NEXTTODO_TOKEN"`} />
      )}
      <Copyable label="把 skill 装给 agent" text="nexttodo skill install" />
      <div className="actions">
        <button className="filled-btn" onClick={onDone}>
          完成
        </button>
      </div>
    </Modal>
  );
}

/** The environment variables the command line, and Codex but in local mode, read: as a shell
 * profile has them, or set for good on Windows. */
function EnvVars({ url, secret }: { url: string; secret: string }) {
  const [os, setOs] = useState<"unix" | "windows">(desktopOs === "Windows" ? "windows" : "unix");
  const unix = os === "unix";
  return (
    <Copyable
      label={url === "local" ? "命令行用的环境变量" : "命令行和 Codex 用的环境变量"}
      hint={unix ? "写进 ~/.zshrc 等 shell 配置" : "运行后重新打开终端"}
      text={unix ? `export NEXTTODO_URL=${url}\nexport NEXTTODO_TOKEN=${secret}` : `setx NEXTTODO_URL "${url}"\nsetx NEXTTODO_TOKEN "${secret}"`}
    >
      <Seg<"unix" | "windows">
        label="系统"
        small
        items={[
          ["unix", "macOS、Linux"],
          ["windows", "Windows"],
        ]}
        value={os}
        onChange={setOs}
      />
    </Copyable>
  );
}

/** Text to copy, such as a token or a command, with the button that copies it; `children` go next to
 * the label. */
function Copyable({ label, hint, text, children }: { label: string; hint?: string; text: string; children?: ReactNode }) {
  const { snack } = useFeedback();
  return (
    <div className="field">
      <div className="copy-head">
        <span>
          {label} {hint && <span className="hint">{hint}</span>}
        </span>
        {children}
      </div>
      <div className="secret">
        <span>{text}</span>
        <Tip label={`复制${label}`}>
          <button className="icon-btn sm" onClick={() => copy(text).then(() => snack(`已复制${label}`))}>
            <Icon of={Copy} size={18} />
          </button>
        </Tip>
      </div>
    </div>
  );
}
