// The signed-in app: the layout for the screen size, settings, and what the session reports.

import { useEffect, useState } from "react";
import { useLocation, useSearchParams } from "wouter";
import { app } from "../api/http";
import type { LogEntry } from "../api/types/LogEntry";
import type { Board } from "../domain/board";
import { type Session, SessionContext, useSession, useUndo } from "../session";
import { useFeedback } from "../ui/feedback";
import { useWide } from "../ui/hooks";
import { Root } from "../ui/Root";
import { EVERY_MS, check, install, restart, usePending } from "../updates";
import { Desktop } from "./Desktop";
import { MarksProvider } from "./marks";
import { Phone } from "./Phone";
import { Settings } from "./Settings";

export function Main({ session, onSignedOut }: { session: Session; onSignedOut(notice?: string): void }) {
  const wide = useWide();
  const [settings, setSettings] = useState(false);
  // Opened from outside the app: quick add from the widget, the menu bar or the shortcut (`/?add`), a
  // task from the widget or a reminder (`/?open=<task>`). Settings and dialogs make way.
  const [params] = useSearchParams();
  const [, navigate] = useLocation();
  const open = params.get("open");
  const outside = !!open || params.has("add");
  if (outside && settings) setSettings(false);
  // On top of a page without details, which closing them goes back to: there may be no page before.
  useEffect(() => {
    if (!open) return;
    navigate("/", { replace: true });
    navigate(`/?t=${open}`, { state: { sheet: true } });
  }, [open, navigate]);
  const signOut = async () => {
    await session.signOut();
    onSignedOut();
  };
  const className = wide ? "app desk" : "app phone";
  return (
    <SessionContext value={session}>
      <MarksProvider>
        <Root className={className} outside={outside}>
          <Events onSignedOut={onSignedOut} />
          <Updates />
          {wide ? (
            <Desktop onSettings={() => setSettings(true)} />
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
      rejected: (_, e) => snack(`保存失败：${e.message}`),
      unsaved: () => snack("无法保存到本机，关闭页面前请保持联网"),
      appFailed: (e) => snack(`${app === "android" ? "提醒和小组件" : "提醒"}更新失败：${e instanceof Error ? e.message : String(e)}`),
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

/** Looks for updates when the app opens, comes back to the front, and while it runs, and tells the
 * user once about one waiting. */
function Updates() {
  const { snack } = useFeedback();
  const pending = usePending();
  useEffect(() => {
    const shown = () => document.visibilityState === "visible" && void check();
    void check();
    document.addEventListener("visibilitychange", shown);
    // The desktop app may stay open, or in the menu bar or tray, for days.
    const timer = setInterval(() => void check(), EVERY_MS);
    return () => {
      document.removeEventListener("visibilitychange", shown);
      clearInterval(timer);
    };
  }, []);
  useEffect(() => {
    if (pending?.kind === "apk") snack({ text: `NextTodo ${pending.version} 可以安装了`, action: "安装", onAction: () => void install(pending.url, snack), ms: 8000 });
    if (pending?.kind === "restart") snack({ text: "新版本已准备好，重启后生效", action: "重启", onAction: () => void restart(snack), ms: 8000 });
  }, [pending, snack]);
  return null;
}

/** `Claude Code 在“求职”中添加了 4 项` */
function describe(entry: LogEntry, board: Board): string {
  const tasks = entry.effects.filter((e) => e.kind === "task");
  const created = tasks.filter((e) => !e.before).map((e) => e.after!);
  if (created.length && created.length === tasks.length) {
    const folders = new Set(created.map((t) => t.folder_id));
    const [only] = folders;
    const where = folders.size > 1 ? "" : `在“${only ? (board.folder(only)?.name ?? "文件夹") : board.folderName(null)}”中`;
    return `${entry.actor} ${where}添加了 ${created.length} 项`;
  }
  const done = tasks.filter((e) => e.before && e.after && e.before.done_at === null && e.after.done_at !== null);
  if (done.length && done.length === tasks.length) return `${entry.actor} 完成了 ${done.length} 项`;
  return `${entry.actor} 进行了 ${entry.effects.length} 处改动`;
}
