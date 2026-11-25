// The signed-in app: the layout for the screen size, the desktop app's panel, or Android's quick add
// card; and what the session reports.

import { useEffect } from "react";
import { useLocalRunner } from "../ai";
import { useLocation, useSearchParams } from "wouter";
import { app } from "../api/http";
import { fetchServerSettings } from "../attach";
import type { LogEntry } from "../api/types/LogEntry";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import type { Board } from "../domain/board";
import { inPanel } from "../desktop";
import { inCard } from "../mobile";
import { type Session, SessionContext, useSession, useUndo } from "../session";
import { forUser, useFeedback } from "../ui/feedback";
import { useWide } from "../ui/hooks";
import { Root } from "../ui/Root";
import { inLocalMode } from "../local";
import { SERVER_EVERY_MS, checkServer, useServerVersions } from "../server-update";
import { EVERY_MS, check, install, newer, restart, usePending } from "../updates";
import { BatchProvider } from "./batch";
import { Card } from "./Card";
import { WhatsNew } from "./Changelog";
import { Desktop } from "./Desktop";
import { MarksProvider } from "./marks";
import { Panel } from "./Panel";
import { Phone } from "./Phone";
import { settingsUrl, useSelection } from "./nav";

export function Main({ session, onSignedOut }: { session: Session; onSignedOut(notice?: string): void }) {
  const wide = useWide();
  // Opened from outside the app: quick add from the widget or the panel opening (`?add`); a task from
  // the widget or a reminder, on the page of what to do next, or from the panel, on the page it shows
  // (`?open=<task>`). Dialogs make way.
  const [params] = useSearchParams();
  const [location, navigate] = useLocation();
  const open = params.get("open");
  const outside = !!open || params.has("add");
  // On top of the page without details, which closing them goes back to: there may be no page before.
  useEffect(() => {
    if (!open) return;
    navigate(location, { replace: true });
    navigate(`${location}?t=${open}`, { state: { sheet: true } });
  }, [open, location, navigate]);
  const signOut = async () => {
    await session.signOut();
    onSignedOut();
  };
  const className = inPanel ? "app phone panel" : inCard ? "app phone card" : wide ? "app desk" : "app phone";
  return (
    <SessionContext value={session}>
      <MarksProvider>
        <Root className={className} outside={outside}>
          <Events onSignedOut={onSignedOut} />
          {/* The main window's, done once; the card hands AI jobs over itself. */}
          {!inPanel && !inCard && (
            <>
              <LocalRunner />
              <Updates />
              <WhatsNew outside={outside} />
              {(session.user.is_admin || app) && !inLocalMode() && <ServerVersions admin={session.user.is_admin} />}
            </>
          )}
          <BatchProvider>{inPanel ? <Panel /> : inCard ? <Card /> : wide ? <Desktop onSignOut={signOut} /> : <Phone onSignOut={signOut} />}</BatchProvider>
        </Root>
      </MarksProvider>
    </SessionContext>
  );
}

/** Who the server's changes for AI jobs are by (`crates/server/src/ai/mod.rs`). */
const AI_ACTOR = "AI 整理";

/** How long ago an agent's change may be to still be announced when it arrives. */
const RECENT_MS = 2 * 60_000;

/** Tells the user about changes the server turned down, changes their agents made, and changes
 * others made that concern them; and notices when the session has ended elsewhere. */
function Events({ onSignedOut }: { onSignedOut(notice?: string): void }) {
  const session = useSession();
  const { snack } = useFeedback();
  const undo = useUndo();
  const { select } = useSelection();
  const [, navigate] = useLocation();
  useEffect(() => {
    session.listen({
      signedOut: () => onSignedOut("登录已失效，请重新登录"),
      rejected: (_, e) => snack(`保存失败：${e.message}`),
      unsaved: () => snack("无法保存到本机，关闭页面前请保持联网"),
      told: (text) => snack(text),
      appFailed: (e) => snack(forUser(app === "android" ? "更新提醒和小组件" : "更新提醒", e)),
      arrived(entries) {
        const board = session.store.getView().board;
        // Not a job starting or failing: its row shows that.
        const told = (e: LogEntry) => e.effects.some((x) => !(x.kind === "ai_job" && x.before && x.after));
        // The latest worth telling.
        for (const e of entries.filter((e) => e.created_at > Date.now() - RECENT_MS).reverse()) {
          if (e.user === board.me) {
            if (e.actor === null || !told(e)) continue;
            return snack({ text: describe(e, board), action: "撤销", onAction: () => undo(e.id), ms: 8000 });
          }
          const notice = concerning(e, board);
          if (notice) return snack({ text: notice.text, action: "查看", onAction: () => (notice.open ? select(notice.open) : navigate("/")), ms: 8000 });
        }
      },
    });
  });
  useEffect(() => {
    void session.check();
    void fetchServerSettings();
  }, [session]);
  return null;
}

/** Has AI organize, with what is set up on this device (ai.ts). */
function LocalRunner() {
  const session = useSession();
  const { snack } = useFeedback();
  useLocalRunner(session.store, session.user.id, (_, e) => snack(forUser("开始 AI 整理", e)));
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
    if (pending?.kind === "apk") snack({ text: `NextTodo ${pending.version} 可以安装`, action: "安装", onAction: () => void install(pending.url, snack), ms: 8000 });
    if (pending?.kind === "restart") snack({ text: "新版本已准备好，重启后生效", action: "重启", onAction: () => void restart(snack), ms: 8000 });
    if (pending?.kind === "sidestore") snack({ text: `NextTodo ${pending.version} 已发布，在 SideStore 里更新`, ms: 8000 });
  }, [pending, snack]);
  return null;
}

/** The latest version told about for each server, and the server's own version told to be older than
 * the app, after these prefixes and the server's address. */
const SERVER_TOLD = "nexttodo.serverUpdateTold:";
const OLDER_TOLD = "nexttodo.olderServerTold:";

/** Looks at the server's version when the app opens, comes back to the front, and while it runs. For
 * its admin, beside the latest release, telling once about each newer (docs/design.md#部署); for
 * others, in an app, telling once of each server version older than the app, which may turn down what
 * the app sends (docs/design.md#app-更新). */
function ServerVersions({ admin }: { admin: boolean }) {
  const { snack } = useFeedback();
  const [, navigate] = useLocation();
  const versions = useServerVersions();
  useEffect(() => {
    const ask = () => void checkServer({ latest: admin });
    const shown = () => document.visibilityState === "visible" && ask();
    ask();
    document.addEventListener("visibilitychange", shown);
    const timer = setInterval(ask, SERVER_EVERY_MS);
    return () => {
      document.removeEventListener("visibilitychange", shown);
      clearInterval(timer);
    };
  }, [admin]);
  useEffect(() => {
    if (!versions) return;
    const tell = (key: string, version: string, text: string) => {
      if (localStorage.getItem(key + versions.of) === version) return;
      localStorage.setItem(key + versions.of, version);
      snack(admin ? { text, action: "查看", onAction: () => navigate(settingsUrl("server")), ms: 8000 } : { text, ms: 8000 });
    };
    const latest = versions.latest;
    if (admin && latest && newer(latest, versions.server)) tell(SERVER_TOLD, latest, `服务器可升级到 ${latest}`);
    if (!admin && newer(import.meta.env.WEB_VERSION, versions.server)) tell(OLDER_TOLD, versions.server, "服务器的版本比 App 旧，部分改动可能无法保存，请联系管理员升级服务器");
  }, [versions, admin, snack, navigate]);
  return null;
}

/** What someone else did that concerns the user, which their other changes don't (docs/design.md#共享):
 * assigning them a task, or completing tasks that leave some of the user's ready; and the task to
 * open, when it is one. */
function concerning(entry: LogEntry, board: Board): { text: string; open: TaskId | null } | null {
  const who = entry.actor ? `${board.name(entry.user)} 的 ${entry.actor}` : board.name(entry.user);
  const changed = entry.effects.flatMap((x) => (x.kind === "task" && x.after ? [{ before: x.before, after: x.after }] : []));
  /** `“装灯具”`, or ` 3 项`. */
  const which = (tasks: Task[]) => (tasks.length > 1 ? ` ${tasks.length} 项` : `“${tasks[0].title}”`);
  const open = (tasks: Task[]) => (tasks.length > 1 ? null : tasks[0].id);
  const assigned = changed.filter(({ before, after }) => after.assignees.includes(board.me) && !before?.assignees.includes(board.me)).map((c) => c.after);
  if (assigned.length) return { text: `${who} 将${which(assigned)}指派给我`, open: open(assigned) };
  // Done, or a repeating task done this time, and so no longer holding up what follows.
  const done = changed
    .filter(({ before, after }) => before && board.holdsUp(before) && !board.holdsUp(after) && (after.done_at !== null || after.completions?.count !== before.completions?.count))
    .map((c) => c.after);
  const ready = new Map(
    done
      .flatMap((t) => board.after(t))
      .filter((t) => board.mine(t) && board.status(t) === "ready")
      .map((t) => [t.id, t]),
  );
  const unlocked = [...ready.values()];
  return unlocked.length ? { text: `${who} 完成了${which(done)}，解锁了${which(unlocked)}`, open: open(unlocked) } : null;
}

/** `Claude Code 在“求职”中添加了 4 项` */
function describe(entry: LogEntry, board: Board): string {
  const tasks = entry.effects.filter((e) => e.kind === "task");
  const created = tasks.filter((e) => !e.before).map((e) => e.after!);
  if (entry.actor === AI_ACTOR && created.length) return `AI 整理完成，添加了 ${created.length} 项`;
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
