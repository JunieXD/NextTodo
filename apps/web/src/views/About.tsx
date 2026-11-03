// The versions running, and an update waiting: see docs/design.md#app-更新.

import { useEffect, useState } from "react";
import { app } from "../api/http";
import { useFeedback } from "../ui/feedback";
import { appVersion, install, restart, usePending } from "../updates";

export function About() {
  const { snack } = useFeedback();
  const pending = usePending();
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    appVersion().then(setVersion, (e) => snack(`读取版本失败：${e}`));
  }, [snack]);
  return (
    <>
      <h2 className="group-title">关于</h2>
      <div className="group">
        <Row label="网页版本" value={import.meta.env.WEB_VERSION} />
        {app && <Row label="App 版本" value={version ?? ""} />}
        {pending && (
          <div className="row">
            <div className="row-inner">
              <span className="folder-main">
                <span>{pending.kind === "apk" ? "有新版本" : "新版本已安装"}</span>
                <span className="folder-sub">{pending.kind === "apk" ? `${pending.version}，下载后由系统安装` : `${pending.version}，重启后生效`}</span>
              </span>
              {pending.kind === "apk" ? (
                <button className="text-btn" onClick={() => void install(pending.url, snack)}>
                  下载安装
                </button>
              ) : (
                <button className="text-btn" onClick={() => void restart()}>
                  重启
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    </>
  );
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
