// Tokens of a kind and a row for each: the devices signed in to the account, and agents' API tokens.

import { KeyRound } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api/http";
import type { TokenInfo } from "../api/types/TokenInfo";
import { ago } from "../domain/dates";
import { useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
/** The tokens of a kind, loaded once the page opens, and taking one away. */
export function useTokens(kind: TokenInfo["kind"], failed: (e: unknown) => void) {
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

export const used = (t: TokenInfo, now: Date) => (t.last_used_at ? `${ago(t.last_used_at, now)}使用` : "未使用");

export function TokenRow({ icon, title, sub, action }: { icon: typeof KeyRound; title: string; sub: string; action?: [string, () => void] }) {
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
