// The versions running, and updates: see docs/design.md#app-更新.

import { ChevronRight, Copy, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { app } from "../api/http";
import { inLocalMode } from "../local";
import { checkServer, useServerVersions } from "../server-update";
import { useSession } from "../session";
import { ago } from "../domain/dates";
import { useCopy } from "../ui/clipboard";
import { Tip, forUser, useFeedback } from "../ui/feedback";
import { Help, QQ_GROUP, REPOSITORY } from "../ui/help";
import { Icon } from "../ui/icons";
import { appVersion, check, failureText, install, newer, restart, useChecked, useChecking, usePending } from "../updates";
import { Changelog } from "./Changelog";

export function About() {
  const { snack } = useFeedback();
  const copy = useCopy();
  const pending = usePending();
  const { user } = useSession();
  const server = useServerVersions();
  const [installed, setInstalled] = useState<string | null>(null);
  const [changelog, setChangelog] = useState(false);
  useEffect(() => {
    appVersion().then(setInstalled, (e) => snack(forUser("读取版本", e)));
  }, [snack]);
  // However lately it was asked: the server may have been upgraded since it was told to be older than
  // the app. Its admin has it in the server's settings.
  const asks = !!app && !inLocalMode() && !user.is_admin;
  useEffect(() => {
    if (asks) void checkServer({ now: true, latest: false });
  }, [asks]);
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
        {asks && server && newer(version, server.server) && (
          <div className="row">
            <div className="row-inner">
              <span className="folder-main">
                <span>服务器版本</span>
                <span className="folder-sub full">比 App 旧，请联系管理员升级服务器</span>
              </span>
              <span className="row-value">{server.server}</span>
            </div>
          </div>
        )}
        {pending && (
          <div className="row">
            <div className="row-inner">
              <span className="folder-main">
                <span>{pending.kind === "restart" ? "新版本已准备好" : "有新版本"}</span>
                <span className="folder-sub">
                  {pending.version}，{{ apk: "下载后由系统安装", restart: "重启后生效", sidestore: "在 SideStore 里更新" }[pending.kind]}
                </span>
              </span>
              {pending.kind === "apk" && (
                <button className="text-btn" onClick={() => void install(pending.url, snack)}>
                  下载安装
                </button>
              )}
              {pending.kind === "restart" && (
                <button className="text-btn" onClick={() => void restart(snack)}>
                  重启
                </button>
              )}
            </div>
          </div>
        )}
        <button className="row" onClick={() => setChangelog(true)}>
          <div className="row-inner">
            <span className="folder-main">
              <span>更新日志</span>
            </span>
            <span className="chev">
              <Icon of={ChevronRight} size={18} />
            </span>
          </div>
        </button>
      </div>
      {changelog && <Changelog onClose={() => setChangelog(false)} />}
      <div className="group">
        <div className="row">
          <div className="row-inner">
            <span className="folder-main">
              <span>QQ 群</span>
              <span className="folder-sub">反馈和交流</span>
            </span>
            <Tip label="复制群号">
              <button className="row-value copy-value" onClick={() => copy(QQ_GROUP.number, "群号")}>
                {QQ_GROUP.number}
                <Icon of={Copy} size={16} />
              </button>
            </Tip>
            <a className="text-btn" href={QQ_GROUP.url} target="_blank" rel="noreferrer">
              加入
            </a>
          </div>
        </div>
        <div className="row">
          <div className="row-inner">
            <span className="folder-main">
              <span>GitHub</span>
              <span className="folder-sub">开源项目，欢迎 Star 支持</span>
            </span>
            <a className="text-btn" href={REPOSITORY} target="_blank" rel="noreferrer">
              打开
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
