// The versions running, and updates: see docs/design.md#app-更新.

import { Copy, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { app } from "../api/http";
import { inLocalMode } from "../local";
import { type ServerVersions, followUpgrade, newer, upgrade, useServerVersions, useUpgrading } from "../server-update";
import { useSession } from "../session";
import { ago } from "../domain/dates";
import { Tip, forUser, useFeedback } from "../ui/feedback";
import { Help, QQ_GROUP } from "../ui/help";
import { Icon } from "../ui/icons";
import { appVersion, check, failureText, install, restart, useChecked, useChecking, usePending } from "../updates";

export function About() {
  const { snack } = useFeedback();
  const pending = usePending();
  const { user } = useSession();
  const server = useServerVersions();
  const [installed, setInstalled] = useState<string | null>(null);
  useEffect(() => {
    appVersion().then(setInstalled, (e) => snack(forUser("读取版本", e)));
  }, [snack]);
  const version = import.meta.env.WEB_VERSION;
  return (
    <>
      <div className="group">
        {app ? (
          // Checks at once, however lately it did.
          <button className="row" onClick={() => void check(true)}>
            <div className="row-inner">
              <span className="folder-main">
                <span>App 版本</span>
                {installed && installed !== version && <span className="folder-sub full">安装包 {installed}</span>}
                <LastCheck />
              </span>
              <span className="row-value">{version}</span>
              <span className="chev">
                <Icon of={RefreshCw} size={16} />
              </span>
            </div>
          </button>
        ) : (
          <Row label="网页版本" value={version} />
        )}
        {user.is_admin && !inLocalMode() && server && <ServerVersion server={server} />}
        {pending && (
          <div className="row">
            <div className="row-inner">
              <span className="folder-main">
                <span>{pending.kind === "apk" ? "有新版本" : "新版本已准备好"}</span>
                <span className="folder-sub">{pending.kind === "apk" ? `${pending.version}，下载后由系统安装` : `${pending.version}，重启后生效`}</span>
              </span>
              {pending.kind === "apk" ? (
                <button className="text-btn" onClick={() => void install(pending.url, snack)}>
                  下载安装
                </button>
              ) : (
                <button className="text-btn" onClick={() => void restart(snack)}>
                  重启
                </button>
              )}
            </div>
          </div>
        )}
      </div>
      <div className="group">
        <div className="row">
          <div className="row-inner">
            <span className="folder-main">
              <span>QQ 群</span>
              <span className="folder-sub">反馈和交流</span>
            </span>
            <Tip label="复制群号">
              <button className="row-value copy-value" onClick={() => navigator.clipboard.writeText(QQ_GROUP.number).then(() => snack("已复制群号"))}>
                {QQ_GROUP.number}
                <Icon of={Copy} size={16} />
              </button>
            </Tip>
            <a className="text-btn" href={QQ_GROUP.url} target="_blank" rel="noreferrer">
              加入
            </a>
          </div>
        </div>
      </div>
      <p className="help-line">
        <Help to="">使用教程</Help>
      </p>
    </>
  );
}

/** The server's version, and upgrading it from here where it can (docs/design.md#部署). */
function ServerVersion({ server }: { server: ServerVersions }) {
  const { confirm, snack } = useFeedback();
  const upgrading = useUpgrading();
  useEffect(() => {
    followUpgrade().catch((e) => console.warn("查询服务器升级失败", e));
  }, []);
  const latest = server.latest !== null && newer(server.latest, server.server) ? server.latest : null;
  const howTo = <Help to="server#升级">查看升级方法</Help>;
  const start = async (version: string) => {
    const body = "服务器会下载约 45 MB，换上新版本时各设备断开几秒。新版本起不来时会自动退回。";
    if (await confirm({ title: `升级到 ${version}？`, body, ok: "升级" })) await upgrade(version).catch((e) => snack(forUser("升级", e)));
  };
  // Once the server can upgrade itself: those that cannot, and those not asked yet, have the way by hand.
  const button = latest && (upgrading?.state === "idle" || upgrading?.state === "failed") && (
    <button className="text-btn" onClick={() => void start(latest)}>
      {upgrading.state === "failed" && upgrading.version === latest ? "重试" : "升级"}
    </button>
  );
  const text =
    upgrading?.state === "downloading" ? (
      `正在下载 ${upgrading.version}，${upgrading.total ? Math.floor((upgrading.received * 100) / upgrading.total) : 0}%`
    ) : upgrading?.state === "switching" || upgrading?.state === "restarting" ? (
      `正在重启，换上 ${upgrading.version}…`
    ) : upgrading?.state === "upgraded" ? (
      `已升级到 ${upgrading.version}`
    ) : upgrading?.state === "lost" ? (
      "服务器没有重新启动，请在服务器上查看（docker compose ps）"
    ) : upgrading?.state === "failed" ? (
      <>
        {upgrading.message}，{howTo}
      </>
    ) : server.latest === null ? (
      "无法查询最新版本"
    ) : !latest ? (
      "已是最新版本"
    ) : button ? (
      `可升级到 ${latest}`
    ) : (
      <>
        可升级到 {latest}，{howTo}
      </>
    );
  return (
    <div className="row">
      <div className="row-inner">
        <span className="folder-main">
          <span>服务器版本</span>
          <span className="folder-sub full">{text}</span>
        </span>
        <span className="row-value">{server.server}</span>
        {button}
      </div>
    </div>
  );
}

/** How the last check for updates went, under the app's version: why it failed, also with an update
 * waiting, which the row below shows. */
function LastCheck() {
  const checking = useChecking();
  const checked = useChecked();
  const pending = usePending();
  const text = checking
    ? "正在检查更新…"
    : !checked
      ? null
      : checked.failure
        ? failureText("检查更新", checked.failure)
        : pending
          ? null
          : checked.staged
            ? "新版本已下载，下次打开时生效"
            : `已是最新版本，${ago(checked.at, new Date())}检查`;
  return text && <span className="folder-sub full">{text}</span>;
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="row">
      <div className="row-inner">
        <span>{label}</span>
        <span className="row-value">{value}</span>
      </div>
    </div>
  );
}
