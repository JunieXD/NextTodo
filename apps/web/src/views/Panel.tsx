// The menu bar panel and the desktop widgets share their contents (docs/desktop.md).

import { AppWindow, CalendarClock, CalendarDays, CalendarRange, Check, ChevronDown, CircleCheck, List, X } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { type ReactNode, useContext, useEffect } from "react";
import { Redirect, useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { desktopWidget, hidePanel, inWidget, openMain, whenRemoved, widgetLabel, widgetPageKey } from "../desktop";
import type { PrivacyLock } from "../lock";
import { useConnection, useSession, useView } from "../session";
import { forUser, RootContext, Tip, useFeedback } from "../ui/feedback";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { BatchBar, BatchHead, useBatch } from "./batch";
import { AgendaBody, CalendarBody, type CalendarView, useCalendarPick, useTimeGridOn } from "./calendar";
import { FolderGlyph } from "./folder-look";
import { FolderBody, NextBody, NextFilter } from "./lists";
import { LockScreen } from "./Lock";
import { folderUrl } from "./nav";
import { QuickAdd } from "./quick-add";
import { FolderName } from "./share";
import { ConnectionNote } from "./status";

const PANEL_PAGE = "nexttodo.panelPage";
const PAGE = widgetLabel ? widgetPageKey(widgetLabel) : PANEL_PAGE;

export const PAGES = [
  { path: "/", name: "接下来", icon: CircleCheck },
  { path: "/agenda", name: "日程", icon: List },
  { path: "/calendar/week", name: "一周", icon: CalendarRange, view: "week" },
  { path: "/calendar/month", name: "月历", icon: CalendarDays, view: "month" },
  { path: "/calendar/time", name: "时间轴", icon: CalendarClock, view: "time" },
] as const;

const availablePath = (path: string, grid: boolean) => (path === "/calendar/time" && !grid ? "/calendar/week" : path);

function folderPage(path: string) {
  return path.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
}

const close = () => {
  if (!inWidget) void hidePanel();
};

/** Older versions had one widget and stored the choice of calendar view separately. */
export function restorePage() {
  const legacy = widgetLabel === "widget-1" ? "nexttodo.widgetPage" : PAGE;
  const calendar = `${legacy}.calendarView`;
  let path = localStorage.getItem(PAGE) ?? localStorage.getItem(legacy) ?? window.nexttodoFirstPage ?? "/";
  if (path === "/calendar") path = localStorage.getItem(calendar) === "time" ? "/calendar/time" : "/calendar/week";
  if (legacy !== PAGE) localStorage.removeItem(legacy);
  localStorage.removeItem(calendar);
  localStorage.setItem(PAGE, path);
  history.replaceState(null, "", path);
}

/** The icon of the page at `path`, as the menu of pages has it. */
export function PanelPageIcon({ path }: { path: string }) {
  const { board } = useView();
  const [grid] = useTimeGridOn();
  const p = PAGES.find((p) => p.path === availablePath(path, grid));
  if (p) return <Icon of={p.icon} size={18} />;
  const folder = folderPage(path);
  return <FolderGlyph board={board} id={folder && board.folder(folder) ? folder : null} />;
}

export function PanelPageName({ path }: { path: string }) {
  const { board } = useView();
  const [grid] = useTimeGridOn();
  const p = PAGES.find((p) => p.path === availablePath(path, grid));
  if (p) return p.name;
  const folder = folderPage(path);
  return path === "/folders" || (folder && board.folder(folder)) ? <FolderName board={board} id={folder} /> : PAGES[0].name;
}

export function Panel() {
  const [location, navigate] = useLocation();
  const { board } = useView();
  const connection = useConnection();
  const batch = useBatch();
  const now = useNow();
  const [grid] = useTimeGridOn();
  const path = availablePath(location, grid);
  const page = PAGES.find((p) => p.path === path);
  const folder = folderPage(path);
  const next = path === "/";
  const agenda = path === "/agenda";
  const view = page && "view" in page ? page.view : null;
  const pick = useCalendarPick(now, view ?? "week");
  const setView = (view: CalendarView) => navigate(`/calendar/${view}`);
  // Links out of the pages a panel hosts belong to the main window.
  const elsewhere = !page && path !== "/folders" && !folder;
  useEffect(() => {
    if (!elsewhere) return;
    void openMain(location);
    history.back();
  }, [elsewhere, location]);
  useEffect(() => {
    if (!elsewhere) localStorage.setItem(PAGE, path);
  }, [elsewhere, path]);
  useEscape(() => (batch.on ? batch.end() : close()));
  const { store } = useSession();
  const { snack } = useFeedback();
  useEffect(
    () =>
      whenRemoved(async () => {
        const saved = await store.saved();
        if (!saved) snack("移除失败：还有改动没有保存，请稍后重试");
        return saved;
      }),
    [store, snack],
  );
  if (elsewhere) return null;
  if (path !== location) return <Redirect to={path} replace />;
  if (folder && !board.folder(folder)) return <Redirect to="/" replace />;
  const mainPath = view ? `/calendar?view=${view}&on=${pick.picked}` : agenda ? `/calendar?view=week&on=${board.now.today}` : path;
  return (
    <>
      <header className="panel-head" data-tauri-drag-region={inWidget ? "deep" : undefined}>
        {batch.on ? (
          <BatchHead />
        ) : (
          <>
            <Pages path={path} />
            <span className="hero-actions">
              {(next || agenda) && <NextFilter />}
              <Tip label="在主窗口打开">
                <button className="icon-btn" onClick={() => void openMain(mainPath)}>
                  <Icon of={AppWindow} />
                </button>
              </Tip>
            </span>
          </>
        )}
      </header>
      <ConnectionNote connection={connection} />
      {!batch.on && (view ? (
        <QuickAdd key="calendar" on={pick.picked} at={pick.at} />
      ) : (
        <QuickAdd key={path} on={agenda ? board.now.today : undefined} filtered={agenda} folder={next || agenda ? undefined : folder} />
      ))}
      {/* Keyed apart from the quick add beside it, which is keyed by the page too. */}
      <div key={`page:${path}`} className="panel-scroll">
        {next ? <NextBody now={now} /> : agenda ? <AgendaBody now={now} /> : view ? <CalendarBody now={now} view={view} onView={setView} pick={pick} compact /> : <FolderBody folder={folder} now={now} />}
      </div>
      {batch.on && <BatchBar />}
    </>
  );
}

/** Esc, once nothing in the panel takes it, a menu or dialog: what usually closes the panel. */
function useEscape(f: () => void) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && !e.defaultPrevented && f();
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  });
}

function Pages({ path }: { path: string }) {
  const { board } = useView();
  const [, navigate] = useLocation();
  const root = useContext(RootContext);
  const { snack } = useFeedback();
  const [grid] = useTimeGridOn();
  const item = (to: string, icon: ReactNode, name: ReactNode, depth = 0) => (
    <DropdownMenu.Item key={to} className="menu-item" style={{ paddingLeft: 12 + depth * 16 }} onSelect={() => navigate(to)}>
      {icon}
      <span className="truncate">{name}</span>
      {path === to && (
        <span className="tick">
          <Icon of={Check} size={18} />
        </span>
      )}
    </DropdownMenu.Item>
  );
  const tree = (id: FolderId | null, depth: number): ReactNode[] => [
    item(folderUrl(id), <FolderGlyph board={board} id={id} />, <FolderName board={board} id={id} />, depth),
    ...board.subfolders(id).flatMap((f) => tree(f.id, depth + 1)),
  ];
  const theirs = board.sharedWithMe();
  const remove = () => {
    if (widgetLabel) void desktopWidget.remove(widgetLabel).catch((e: unknown) => snack(forUser("移除桌面小组件", e)));
  };
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="panel-pages">
        <span className="truncate">
          <PanelPageName path={path} />
        </span>
        <Icon of={ChevronDown} size={18} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal container={root}>
        <DropdownMenu.Content className="menu panel-menu" align="start" sideOffset={4}>
          {PAGES.filter((p) => grid || p.path !== "/calendar/time").map((p) => item(p.path, <Icon of={p.icon} />, p.name))}
          <DropdownMenu.Separator className="menu-sep" />
          {tree(null, 0)}
          {theirs.length > 0 && (
            <>
              <DropdownMenu.Label className="menu-label">共享给我</DropdownMenu.Label>
              {theirs.flatMap((f) => tree(f.id, 0))}
            </>
          )}
          {widgetLabel && (
            <>
              <DropdownMenu.Separator className="menu-sep" />
              <DropdownMenu.Item className="menu-item danger-text" onSelect={remove}>
                <Icon of={X} />
                移除这个小组件
              </DropdownMenu.Item>
            </>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Locked (docs/design.md#隐私锁): unlocked here as in the main window, closed as the panel is. */
export function PanelLocked({ lock }: { lock: PrivacyLock }) {
  useEscape(close);
  return <LockScreen lock={lock} />;
}

/** Signed out, or local mode would not start: the main window is where to see to it, and a sign-in
 * there loads the panel again (main.tsx). */
export function PanelOut({ failed }: { failed?: string }) {
  useEscape(close);
  return (
    <div className="panel-out">
      {failed && <p>{failed}</p>}
      <button className="filled-btn" onClick={() => void openMain("/")}>
        {failed ? "打开 NextTodo" : "打开 NextTodo 登录"}
      </button>
    </div>
  );
}
