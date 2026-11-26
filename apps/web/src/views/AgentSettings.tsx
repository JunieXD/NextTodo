// Connecting agents (docs/agent.md): API tokens, the command line, and how to set them up.

import { KeyRound, Plus, SquareTerminal } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { ApiError, api, app, desktopOs, serverUrl } from "../api/http";
import type { CreatedToken } from "../api/types/CreatedToken";
import type { Scope } from "../api/types/Scope";
import { type CliStatus, cli } from "../desktop";
import { checkTokenName } from "../domain/text";
import * as local from "../local";
import { Copyable, Seg } from "../ui/controls";
import { Modal, forUser, useFailure, useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";
import { Icon } from "../ui/icons";
import { TokenRow, useTokens, used } from "./tokens";

export function Agent() {
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
  const form = useId();
  return (
    <Modal
      title="生成令牌"
      onClose={onClose}
      footer={
        <>
          {error && <p className="form-error">{error}</p>}
          <button type="button" className="text-btn" onClick={onClose}>
            取消
          </button>
          <button className="filled-btn" form={form}>
            生成
          </button>
        </>
      }
    >
      <form id={form} onSubmit={submit}>
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
      </form>
    </Modal>
  );
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
    <Modal
      title="令牌已生成"
      onClose={onDone}
      onBack={onDone}
      footer={
        <button className="filled-btn" onClick={onDone}>
          完成
        </button>
      }
    >
      <p className="empty">
        “{name}”的令牌只显示这一次。下面的写法已填好{inLocal ? "令牌，须先安装命令行" : "服务器地址和令牌"}，复制即可。
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
