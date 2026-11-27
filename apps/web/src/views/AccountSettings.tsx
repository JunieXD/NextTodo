// The account's settings (docs/ui/settings.md): the display name, the password, the devices signed in, signing out, and backups.

import { ChevronRight, Monitor } from "lucide-react";
import { useId, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { ApiError, OFFLINE, api, fetchExport } from "../api/http";
import type { ImportBackup } from "../api/types/ImportBackup";
import type { User } from "../api/types/User";
import { isoDate } from "../domain/dates";
import { checkDisplayName } from "../domain/text";
import { useSession, useUndo } from "../session";
import { Modal, forUser, useFailure, useFeedback } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { readBackup, sendImport, uploadBackup } from "../transfer";
import { saveFile } from "./attachments";
import { TokenRow, useTokens, used } from "./tokens";
import { Action } from "../ui/controls";

export function Account({ onSignOut }: { onSignOut(): void }) {
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
      <Backups />
      <div className="group danger" style={{ marginTop: 24 }}>
        <button className="row" onClick={onSignOut}>
          <div className="row-inner">退出登录</div>
        </button>
      </div>
      {password && <PasswordDialog onClose={() => setPassword(false)} />}
    </>
  );
}

/** The account's own data as a backup, and a backup added to it (docs/local.md#搬到服务器和搬回本机),
 * each showing how it goes meanwhile. */
function Backups() {
  const { goingAsNow } = useSession();
  const { confirm, snack } = useFeedback();
  const undo = useUndo();
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState<string | null>(null);
  const pick = useRef<HTMLInputElement>(null);
  const exportData = async () => {
    setExporting(true);
    try {
      const { name, file } = await fetchExport(isoDate(new Date()));
      const said = await saveFile({ id: uuidv7(), name, type: "application/x-tar" }, file);
      if (said) snack(said);
    } catch (e) {
      snack(forUser("导出备份", e));
    } finally {
      setExporting(false);
    }
  };
  const importData = async (file: File) => {
    let uploaded: ImportBackup;
    const current = goingAsNow();
    try {
      const backup = await readBackup(file);
      const body = "备份中的文件夹和事项将加入这个账号的根目录，原有数据不变，导入后可撤销。";
      if (!(await confirm({ title: "导入备份？", body, ok: "导入" }))) return;
      setImporting("正在上传");
      uploaded = await uploadBackup(backup, (done, all) => setImporting(`已上传 ${done}/${all}`), current);
    } catch (e) {
      setImporting(null);
      return snack(forUser("导入备份", e));
    }
    await send(uploaded, current);
  };
  const send = async (uploaded: ImportBackup, current: () => boolean) => {
    setImporting("正在导入");
    try {
      const made = await sendImport(uploaded, current);
      snack({ text: `已导入 ${made.tasks} 个事项、${made.folders} 个文件夹`, action: "撤销", onAction: () => undo(uploaded.id) });
    } catch (e) {
      // Unanswered, it may have gone in: sent again as the same change, it goes in once.
      const again = e instanceof ApiError && e.code === OFFLINE;
      snack(again ? { text: forUser("导入备份", e), action: "重试", onAction: () => void send(uploaded, current) } : forUser("导入备份", e));
    } finally {
      setImporting(null);
    }
  };
  return (
    <>
      <h2 className="group-title">备份</h2>
      <div className="group">
        <Action label="导出备份" value={exporting ? "正在导出" : undefined} onClick={() => !exporting && void exportData()} />
        <Action label="导入备份" value={importing ?? undefined} onClick={() => importing === null && pick.current!.click()} />
      </div>
      <p className="group-note">备份可在本机模式中恢复，或导入其他账号；本机模式的备份也可导入这里。</p>
      <input
        ref={pick}
        type="file"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0];
          // Choosing the same file again is a change too.
          e.target.value = "";
          if (file) void importData(file);
        }}
      />
    </>
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
  const form = useId();
  return (
    <Modal
      title="修改密码"
      onClose={onClose}
      footer={
        <>
          {error && (
            <p className="form-error" role="alert">
              {error.message}
            </p>
          )}
          <button type="button" className="text-btn" onClick={onClose}>
            取消
          </button>
          <button className="filled-btn" form={form}>
            修改
          </button>
        </>
      }
    >
      <form id={form} onSubmit={submit}>
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
      </form>
    </Modal>
  );
}
