// The desktop app's menu bar or tray panel: what is next or a folder, ticked off and added to without
// the main window (docs/desktop.md#菜单栏小窗); the desktop widget too, which it does not close and which
// is moved by its top row (docs/desktop.md#桌面小组件).

import { AppWindow, CalendarClock, CalendarDays, Check, ChevronDown, CircleCheck } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { type ReactNode, useContext, useEffect } from "react";
import { Redirect, useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { hidePanel, inWidget, openMain } from "../desktop";
import type { PrivacyLock } from "../lock";
import { useConnection, useView } from "../session";
import { LockScreen } from "./Lock";
import { RootContext, Tip } from "../ui/feedback";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { BatchBar, BatchHead, useBatch } from "./batch";
import { CalendarBody, type CalendarView, useCalendarPick, useCalendarView, useTimeGridOn } from "./calendar";
import { FolderBody, NextBody, NextFilter, QuickAdd } from "./lists";
import { folderUrl } from "./nav";
import { FolderGlyph } from "./folder-look";
import { FolderName } from "./share";
import { ConnectionNote } from "./status";

/** The page it shows, remembered on this device, the panel's apart from the widget's; and on the
 * calendar's, the week or the time grid, apart from the main window's. */
const PAGE = inWidget ? "nexttodo.widgetPage" : "nexttodo.panelPage";
const CALENDAR_VIEW = `${PAGE}.calendarView`;

/** What closes the panel; the widget stays. */
const close = () => {
  if (!inWidget) void hidePanel();
};

/** Back on the page it showed last, before it first renders. */
export const restorePage = () => history.replaceState(null, "", localStorage.getItem(PAGE) ?? "/");

export function Panel() {
  const [location] = useLocation();
  const { board } = useView();
  const connection = useConnection();
  const batch = useBatch();
  const now = useNow();
  const folder = location.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
  const next = location === "/";
  const calendar = location === "/calendar";
  const [stored, setView] = useCalendarView(CALENDAR_VIEW);
  const view = stored === "time" ? "time" : "week";
  const pick = useCalendarPick(now, view);
  // Anywhere else, the settings a link leads to say, is the main window's.
  const elsewhere = !next && !calendar && location !== "/folders" && !folder;
  useEffect(() => {
    if (!elsewhere) return;
    void openMain(location);
    history.back();
  }, [elsewhere, location]);
  useEffect(() => {
    if (!elsewhere) localStorage.setItem(PAGE, location);
  }, [elsewhere, location]);
  useEscape(() => (batch.on ? batch.end() : close()));
  if (elsewhere) return null;
  if (folder && !board.folder(folder)) return <Redirect to="/" replace />;
  return (
    <>
      <header className="panel-head" data-tauri-drag-region={inWidget ? "deep" : undefined}>
        {batch.on ? (
          <BatchHead />
        ) : (
          <>
            <Pages page={next ? "next" : calendar ? view : folder} onView={setView} />
            <span className="hero-actions">
              {next && <NextFilter />}
              <Tip label="在主窗口打开">
                <button className="icon-btn" onClick={() => void openMain(calendar ? `/calendar?view=${view}&on=${pick.picked}` : location)}>
                  <Icon of={AppWindow} />
                </button>
              </Tip>
            </span>
          </>
        )}
      </header>
      <ConnectionNote connection={connection} />
      {!batch.on && (calendar ? <QuickAdd key="calendar" on={pick.picked} at={pick.at} /> : <QuickAdd key={location} folder={next ? undefined : folder} />)}
      <div className="panel-scroll">
        {next ? <NextBody now={now} /> : calendar ? <CalendarBody now={now} view={view} onView={setView} pick={pick} /> : <FolderBody folder={folder} now={now} />}
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

/** Says which page it shows and switches to another: what is next, the calendar's week or time grid,
 * the latter while the settings offer it, or any folder, those others share in a group after the
 * user's own. */
function Pages({ page, onView }: { page: "next" | "week" | "time" | FolderId | null; onView(v: CalendarView): void }) {
  const { board } = useView();
  const [, navigate] = useLocation();
  const root = useContext(RootContext);
  const [grid] = useTimeGridOn();
  const item = (key: string, to: string, icon: ReactNode, name: ReactNode, on: boolean, depth = 0, view?: CalendarView) => (
    <DropdownMenu.Item
      key={key}
      className="menu-item"
      style={{ paddingLeft: 12 + depth * 16 }}
      onSelect={() => {
        if (view) onView(view);
        navigate(to);
      }}
    >
      {icon}
      <span className="truncate">{name}</span>
      {on && (
        <span className="tick">
          <Icon of={Check} size={18} />
        </span>
      )}
    </DropdownMenu.Item>
  );
  const tree = (id: FolderId | null, depth: number): ReactNode[] => [
    item(id ?? "root", folderUrl(id), <FolderGlyph board={board} id={id} />, <FolderName board={board} id={id} />, page === id, depth),
    ...board.subfolders(id).flatMap((f) => tree(f.id, depth + 1)),
  ];
  const theirs = board.sharedWithMe();
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="panel-pages">
        <span className="truncate">{page === "next" ? "接下来" : page === "week" ? "一周" : page === "time" ? "时间轴" : <FolderName board={board} id={page} />}</span>
        <Icon of={ChevronDown} size={18} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal container={root}>
        <DropdownMenu.Content className="menu panel-menu" align="start" sideOffset={4}>
          {item("next", "/", <Icon of={CircleCheck} />, "接下来", page === "next")}
          {item("week", "/calendar", <Icon of={CalendarDays} />, "一周", page === "week", 0, "week")}
          {grid && item("time", "/calendar", <Icon of={CalendarClock} />, "时间轴", page === "time", 0, "time")}
          <DropdownMenu.Separator className="menu-sep" />
          {tree(null, 0)}
          {theirs.length > 0 && (
            <>
              <DropdownMenu.Label className="menu-label">共享给我</DropdownMenu.Label>
              {theirs.flatMap((f) => tree(f.id, 0))}
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
