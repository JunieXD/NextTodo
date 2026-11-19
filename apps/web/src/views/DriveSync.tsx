// Syncing local mode's data with other devices through a cloud drive (docs/local.md#借网盘同步).

import { useCallback, useEffect, useState } from "react";
import { ApiError, api } from "../api/http";
import type { DriveConfig } from "../api/types/DriveConfig";
import type { DriveConfigView } from "../api/types/DriveConfigView";
import type { DriveSync } from "../api/types/DriveSync";
import { ago } from "../domain/dates";
import * as local from "../local";
import { useSession } from "../session";
import { Seg } from "../ui/controls";
import { Modal, forUser, useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";

/** How often the settings look at how syncing goes while they show it. */
const LOOK_MS = 5000;

/** How syncing goes, as the app's server says, looked at again every few seconds when `watch`. */
export function useDriveSync(watch: boolean): [DriveSync | null, () => Promise<void>] {
  const { snack } = useFeedback();
  const [sync, setSync] = useState<DriveSync | null>(null);
  const load = useCallback(() => api<DriveSync>("GET", "/local/sync").then(setSync, (e) => snack(forUser("读取同步的状态", e))), [snack]);
  useEffect(() => {
    void load();
    if (!watch) return;
    const timer = setInterval(() => void load(), LOOK_MS);
    return () => clearInterval(timer);
  }, [load, watch]);
  return [sync, load];
}

/** The drive, in a few words: its host or bucket, and the folder. */
const where = (d: DriveConfigView) => `${d.kind === "webdav" ? new URL(d.url).host : d.bucket}/${d.folder}`;

/** How syncing goes, in a line: on the settings' card, and over the dialog's buttons. */
export function syncLine(sync: DriveSync, now: Date): string {
  if (!sync.drive || !sync.status) return sync.stopped ? `同步已停止：${sync.stopped}` : "数据只存在这台设备上，不同步";
  const { enabling, problem, synced_at } = sync.status;
  if (problem) return `同步受阻：${problem}`;
  if (enabling) return `正在把数据传上网盘 ${where(sync.drive)}`;
  return `借网盘同步到 ${where(sync.drive)}，${synced_at === null ? "还没同步过" : `${ago(synced_at, now)}同步`}`;
}

/** Turning syncing on, or how it goes and stopping it while it is on. */
export function DriveSyncDialog({ onClose }: { onClose(): void }) {
  const [sync, load] = useDriveSync(true);
  if (!sync) return null;
  return (
    <Modal title="借网盘同步" onClose={onClose}>
      {sync.drive ? <Syncing sync={sync} onStopped={() => void load()} /> : <TurnOn stopped={sync.stopped} onDone={onClose} />}
    </Modal>
  );
}

function Syncing({ sync, onStopped }: { sync: DriveSync; onStopped(): void }) {
  const { confirm, snack } = useFeedback();
  const [busy, setBusy] = useState(false);
  const stop = async () => {
    const body = "这台设备上的数据留着，之后的改动只在这台设备上，网盘里的不动。再开启时，会用网盘里的数据替换这台设备上的数据。";
    if (!(await confirm({ title: "停止同步？", body, ok: "停止同步", danger: true }))) return;
    const anyway = (body: string) => confirm({ title: "仍然停止同步？", body, ok: "仍然停止", danger: true });
    setBusy(true);
    try {
      if (await local.stopSyncing(anyway)) onStopped();
    } catch (e) {
      snack(forUser("停止同步", e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <p className={sync.status?.problem ? "form-error" : "empty"}>{syncLine(sync, new Date())}</p>
      <div className="actions">
        <button className="text-btn danger-text" onClick={stop} disabled={busy}>
          {busy ? "正在停止…" : "停止同步"}
        </button>
      </div>
    </>
  );
}

type Kind = DriveConfig["kind"];

const JIANGUOYUN = "https://dav.jianguoyun.com/dav/";

function TurnOn({ stopped, onDone }: { stopped: string | null; onDone(): void }) {
  const session = useSession();
  const { confirm, snack } = useFeedback();
  const [kind, setKind] = useState<Kind>("webdav");
  const [dav, setDav] = useState({ url: JIANGUOYUN, user: "", password: "" });
  const [s3, setS3] = useState({ endpoint: "", region: "", bucket: "", access_key: "", secret_key: "", path_style: false });
  const [folder, setFolder] = useState("NextTodo");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ message: string; field?: string } | null>(stopped ? { message: `同步已停止：${stopped}` } : null);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const config: DriveConfig = kind === "webdav" ? { kind, ...dav, folder } : { kind, ...s3, folder };
    setBusy(true);
    setError(null);
    try {
      const replace = (body: string) => confirm({ title: "用网盘里的数据替换这台设备上的全部数据？", body, ok: "替换", danger: true });
      if (await local.syncThrough(config, replace, session)) {
        snack("已开启借网盘同步");
        onDone();
      }
    } catch (e) {
      if (!(e instanceof ApiError)) return snack(forUser("开启同步", e));
      setError({ message: e.message, field: e.field });
    } finally {
      setBusy(false);
    }
  };
  const field = (name: string, label: string, value: string, set: (v: string) => void, hint?: string, secret = false) => (
    <label className={`field${error?.field === name ? " error" : ""}`}>
      <span>
        {label} {hint && <span className="hint">{hint}</span>}
      </span>
      <input
        type={secret ? "password" : "text"}
        autoComplete="off"
        autoCapitalize="off"
        spellCheck={false}
        value={value}
        onChange={(e) => (set(e.target.value), setError(null))}
        aria-invalid={error?.field === name}
      />
    </label>
  );
  return (
    <form onSubmit={submit}>
      <p className="empty">
        几台设备的本机模式通过网盘同步，不用服务器。改动几十秒内到别的设备。<Help to="local#drive">如何设置</Help>
      </p>
      <div style={{ margin: "0 16px 14px" }}>
        <Seg<Kind>
          label="网盘"
          items={[
            ["webdav", "WebDAV"],
            ["s3", "S3"],
          ]}
          value={kind}
          onChange={setKind}
        />
      </div>
      {kind === "webdav" ? (
        <>
          {field("url", "地址", dav.url, (url) => setDav({ ...dav, url }), "坚果云不用改")}
          {field("user", "用户名", dav.user, (user) => setDav({ ...dav, user }))}
          {field("password", "密码", dav.password, (password) => setDav({ ...dav, password }), "坚果云要用应用密码", true)}
        </>
      ) : (
        <>
          {field("endpoint", "接口地址", s3.endpoint, (endpoint) => setS3({ ...s3, endpoint }), "如 https://s3.example.com")}
          {field("region", "区域", s3.region, (region) => setS3({ ...s3, region }), "不填为 us-east-1")}
          {field("bucket", "存储桶", s3.bucket, (bucket) => setS3({ ...s3, bucket }))}
          {field("access_key", "Access Key ID", s3.access_key, (access_key) => setS3({ ...s3, access_key }))}
          {field("secret_key", "Secret Access Key", s3.secret_key, (secret_key) => setS3({ ...s3, secret_key }), undefined, true)}
          <div className="field">
            <span>
              地址风格 <span className="hint">MinIO 这类自建的选路径</span>
            </span>
            <Seg<"host" | "path">
              label="地址风格"
              items={[
                ["host", "虚拟主机"],
                ["path", "路径"],
              ]}
              value={s3.path_style ? "path" : "host"}
              onChange={(style) => setS3({ ...s3, path_style: style === "path" })}
            />
          </div>
        </>
      )}
      {field("folder", "文件夹", folder, setFolder, "不存在就新建")}
      {error && (
        <p className="form-error" role="alert">
          {error.message}
        </p>
      )}
      <div className="actions">
        <button type="button" className="text-btn" onClick={onDone}>
          取消
        </button>
        <button className="filled-btn" disabled={busy}>
          {busy ? "正在连接网盘…" : "开启"}
        </button>
      </div>
    </form>
  );
}
