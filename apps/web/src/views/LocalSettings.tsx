// Local mode's settings (docs/local.md): syncing through a drive, backups, and moving to a server.

import { useState } from "react";
import * as local from "../local";
import { useSession } from "../session";
import { forUser, useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";
import { DriveSyncDialog, useDriveSync } from "./DriveSync";
import { Action } from "../ui/controls";
/** In place of the account: the data is on this device, syncing it with others through a drive, its
 * backups, and how to move to a server. */
export function LocalMode() {
  const session = useSession();
  const { confirm, snack } = useFeedback();
  const [sync, load] = useDriveSync(false);
  const [syncing, setSyncing] = useState(false);
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
    const body = "本机数据保留在这台设备上，再次选择“在本机使用”即可继续使用。NextTodo 将重新启动。";
    if (!(await confirm({ title: "改用服务器？", body, ok: "改用服务器" }))) return;
    await local.leave(session).catch((e) => snack(forUser("改用服务器", e)));
  };
  return (
    <>
      <p className="empty">
        {sync?.drive ? "数据存在这台设备上，通过网盘与其他设备同步。" : "数据只存在这台设备上，可通过网盘与其他设备同步。"}请定期备份，换设备时用备份恢复。
        <Help to="local">说明</Help>
      </p>
      <div className="group">
        <Action label="借网盘同步" value={sync ? (sync.drive ? "已开启" : "未开启") : undefined} onClick={() => setSyncing(true)} />
        <Action label="备份" onClick={backup} />
        <Action label="恢复" onClick={restore} />
        <Action label="改用服务器" onClick={leave} />
      </div>
      {syncing && <DriveSyncDialog onClose={() => (setSyncing(false), void load())} />}
    </>
  );
}
