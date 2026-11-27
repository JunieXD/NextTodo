// The server's own settings, its admin's: its version and upgrading it, invites, and attachments; in
// local mode the attachments alone, the server being the app's (docs/ui/settings.md).

import { Copy, Mail } from "lucide-react";
import { useEffect, useState } from "react";
import { ApiError, api, serverUrl } from "../api/http";
import type { Invite } from "../api/types/Invite";
import type { ServerSettings } from "../api/types/ServerSettings";
import { fetchServerSettings, keepServerSettings, serverSettings } from "../attach";
import { ago } from "../domain/dates";
import { inLocalMode } from "../local";
import { type ServerVersions, checkServer, followUpgrade, upgrade, useServerVersions, useUpgrading } from "../server-update";
import { useSession } from "../session";
import { copyText, useCopy } from "../ui/clipboard";
import { Switch } from "../ui/controls";
import { Tip, forUser, useFailure, useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";
import { Icon } from "../ui/icons";
import { newer } from "../updates";

export function ServerPage() {
  const { user } = useSession();
  const admin = user.is_admin && !inLocalMode();
  const server = useServerVersions();
  // However lately it was asked: an admin come to look may have just heard of a release.
  useEffect(() => {
    if (admin) void checkServer({ now: true, latest: true });
  }, [admin]);
  if (!admin) return <AttachmentSettings />;
  return (
    <>
      {server && (
        <>
          <h2 className="group-title">版本</h2>
          <div className="group">
            <ServerVersion server={server} />
          </div>
        </>
      )}
      <h2 className="group-title">邀请注册</h2>
      <Invites />
      <h2 className="group-title">附件</h2>
      <AttachmentSettings />
    </>
  );
}

/** The server's version, and upgrading it from here where it can (docs/design.md#部署). */
function ServerVersion({ server }: { server: ServerVersions }) {
  const { confirm, snack } = useFeedback();
  const upgrading = useUpgrading();
  useEffect(() => {
    followUpgrade().catch((e: unknown) => {
      // Not reached or turned down, it shows no upgrade under way, as before.
      if (!(e instanceof ApiError)) throw e;
      console.warn("查询服务器升级失败", e);
    });
  }, []);
  const latest = server.latest !== null && newer(server.latest, server.server) ? server.latest : null;
  const howTo = <Help to="server#升级">查看升级方法</Help>;
  const start = async (version: string) => {
    const body = "服务器将下载约 45 MB，更新时各设备断开几秒；新版本无法启动时自动回退。";
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
      `已是最新版本，${ago(server.at, new Date())}检查`
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

/** The link that opens registration with an invite code: on the server, which in the app is not
 * where the page comes from. */
const inviteLink = (code: string) => `${serverUrl() || location.origin}/?invite=${code}`;

function Invites() {
  const [invites, setInvites] = useState<Invite[] | null>(null);
  const [error, failed] = useFailure();
  const { snack } = useFeedback();
  useEffect(() => {
    api<Invite[]>("GET", "/invites").then(setInvites, failed);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- loads once when opened
  }, []);
  const copy = useCopy();
  const invite = async () => {
    const made = await api<Invite>("POST", "/invites").catch(failed);
    if (!made) return;
    setInvites((list) => [made, ...(list ?? [])]);
    // The invite is made whether or not the link reaches the clipboard: a click a while back may no
    // longer allow copying, on a page on plain HTTP.
    await copyText(inviteLink(made.code)).then(
      () => snack("已复制邀请链接"),
      () => snack("邀请码已生成，点它右边的按钮复制链接"),
    );
  };
  const now = new Date();
  return (
    <>
      {error && <p className="form-error">{error}</p>}
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
                  <button className="icon-btn sm" onClick={() => copy(inviteLink(i.code), "邀请链接")}>
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
      <p className="group-note">每人一个账号，数据彼此独立。每个邀请码只能注册一个账号。</p>
    </>
  );
}

/** Whether photos are made smaller before they are uploaded (docs/design.md#附件), for everyone on the
 * server. As last fetched, then as the server has it. */
function AttachmentSettings() {
  const { snack } = useFeedback();
  const [on, setOn] = useState(() => serverSettings().compress_images);
  useEffect(() => {
    void fetchServerSettings().then(() => setOn(serverSettings().compress_images));
  }, []);
  const change = async (to: boolean) => {
    try {
      const s = await api<ServerSettings>("PUT", "/settings", { compress_images: to } satisfies ServerSettings);
      keepServerSettings(s);
      setOn(s.compress_images);
    } catch (e) {
      snack(forUser("修改设置", e));
    }
  };
  return (
    <div className="group">
      <Switch label="压缩图片" sub="上传前将 JPEG 照片缩小到长边 2560 像素以内，并去除拍摄地点等信息。对所有用户生效。" on={on} onChange={(to) => void change(to)} />
    </div>
  );
}
