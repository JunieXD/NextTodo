// The versions running, and updates: see docs/design.md#app-更新.

import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { app } from "../api/http";
import { inLocalMode } from "../local";
import { newer, useServerVersions } from "../server-update";
import { useSession } from "../session";
import { ago } from "../domain/dates";
import { forUser, useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";
import { Icon } from "../ui/icons";
import { appVersion, check, failureText, install, restart, useChecked, useChecking, usePending } from "../updates";

export function About() {
  const { snack } = useFeedback();
  const pending = usePending();
  const { user } = useSession();
  const server = useServerVersions();
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    appVersion().then(setVersion, (e) => snack(forUser("读取版本", e)));
  }, [snack]);
  return (
    <>
      <h2 className="group-title">关于</h2>
      <div className="group">
        <Row label="网页版本" value={import.meta.env.WEB_VERSION} />
        {app && (
          // Checks at once, however lately it did.
          <button className="row" onClick={() => void check(true)}>
            <div className="row-inner">
              <span className="folder-main">
                <span>App 版本</span>
                <LastCheck />
              </span>
              <span className="row-value">{version}</span>
              <span className="chev">
                <Icon of={RefreshCw} size={16} />
              </span>
            </div>
          </button>
        )}
        {user.is_admin && !inLocalMode() && server && (
          <div className="row">
            <div className="row-inner">
              <span className="folder-main">
                <span>服务器版本</span>
                <span className="folder-sub full">
                  {server.latest === null ? (
                    "无法查询最新版本"
                  ) : newer(server.latest, server.server) ? (
                    <>
                      可升级到 {server.latest}，<Help to="server#升级">查看升级方法</Help>
                    </>
                  ) : (
                    "已是最新版本"
                  )}
                </span>
              </span>
              <span className="row-value">{server.server}</span>
            </div>
          </div>
        )}
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
