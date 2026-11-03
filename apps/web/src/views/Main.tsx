// The signed-in app: the layout for the screen size, settings, and what the session reports.

import { useEffect, useState } from "react";
import type { LogEntry } from "../api/types/LogEntry";
import type { Board } from "../domain/board";
import { type Session, SessionContext, useSession, useUndo } from "../session";
import { useFeedback } from "../ui/feedback";
import { useWide } from "../ui/hooks";
import { Root } from "../ui/Root";
import { Desktop } from "./Desktop";
import { MarksProvider } from "./marks";
import { Phone } from "./Phone";
import { Settings } from "./Settings";

export function Main({ session, onSignedOut }: { session: Session; onSignedOut(notice?: string): void }) {
  const wide = useWide();
  const [settings, setSettings] = useState(false);
  const [sideOpen, setSideOpen] = useState(true);
  const [paneOpen, setPaneOpen] = useState(true);
  const signOut = async () => {
    await session.signOut();
    onSignedOut();
  };
  const className = wide ? `app desk${sideOpen ? "" : " side-closed"}${paneOpen ? "" : " pane-closed"}` : "app phone";
  return (
    <SessionContext value={session}>
      <MarksProvider>
        <Root className={className}>
          <Events onSignedOut={onSignedOut} />
          {wide ? (
            <Desktop sideOpen={sideOpen} paneOpen={paneOpen} toggleSide={() => setSideOpen(!sideOpen)} togglePane={() => setPaneOpen(!paneOpen)} onSettings={() => setSettings(true)} />
          ) : (
            <Phone onSettings={() => setSettings(true)} />
          )}
          {settings && <Settings onClose={() => setSettings(false)} onSignOut={signOut} />}
        </Root>
      </MarksProvider>
    </SessionContext>
  );
}

/** How long ago an agent's change may be to still be announced when it arrives. */
const RECENT_MS = 2 * 60_000;

/** Tells the user about changes the server turned down and changes agents made, and notices when
 * the session has ended elsewhere. */
function Events({ onSignedOut }: { onSignedOut(notice?: string): void }) {
  const session = useSession();
  const { snack } = useFeedback();
  const undo = useUndo();
  useEffect(() => {
    session.listen({
      signedOut: () => onSignedOut("登录已失效，请重新登录"),
      rejected: (_, e) => snack(`没能保存：${e.message}`),
      unsaved: () => snack("没能存到这台设备上，关掉页面前请保持联网"),
      appFailed: (e) => snack(`提醒和小组件没能更新：${e instanceof Error ? e.message : String(e)}`),
      arrived(entries) {
        const last = entries.findLast((e) => e.actor !== null && e.effects.length > 0 && e.created_at > Date.now() - RECENT_MS);
        if (last) snack({ text: describe(last, session.store.getView().board), action: "撤销", onAction: () => undo(last.id), ms: 8000 });
      },
    });
  });
  useEffect(() => {
    void session.check();
  }, [session]);
  return null;
}

/** `Claude Code 在“求职”里添加了 4 项` */
function describe(entry: LogEntry, board: Board): string {
  const tasks = entry.effects.filter((e) => e.kind === "task");
  const created = tasks.filter((e) => !e.before).map((e) => e.after!);
  if (created.length && created.length === tasks.length) {
    const folders = new Set(created.map((t) => t.folder_id));
    const [only] = folders;
    const where = folders.size > 1 ? "" : `在“${only ? (board.folder(only)?.name ?? "文件夹") : "根目录"}”里`;
    return `${entry.actor} ${where}添加了 ${created.length} 项`;
  }
  const done = tasks.filter((e) => e.before && e.after && e.before.done_at === null && e.after.done_at !== null);
  if (done.length && done.length === tasks.length) return `${entry.actor} 完成了 ${done.length} 项`;
  return `${entry.actor} 做了 ${entry.effects.length} 处改动`;
}
