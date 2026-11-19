// The desktop app's menu bar or tray panel: what is next or a folder, ticked off and added to without
// the main window (docs/desktop.md#菜单栏小窗).

import { AppWindow, Check, ChevronDown, CircleCheck, FolderRoot } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { type ReactNode, useContext, useEffect } from "react";
import { Redirect, useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { hidePanel, openMain } from "../desktop";
import { useConnection, useView } from "../session";
import { RootContext, Tip } from "../ui/feedback";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { BatchBar, BatchHead, useBatch } from "./batch";
import { FolderBody, NextBody, NextFilter, QuickAdd } from "./lists";
import { folderUrl } from "./nav";
import { FolderGlyph } from "./folder-look";
import { FolderName } from "./share";
import { ConnectionNote } from "./status";

/** The page it shows, remembered on this device. */
const PAGE = "nexttodo.panelPage";

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
  // Anywhere else, the settings a link leads to say, is the main window's.
  const elsewhere = !next && location !== "/folders" && !folder;
  useEffect(() => {
    if (!elsewhere) return;
    void openMain(location);
    history.back();
  }, [elsewhere, location]);
  useEffect(() => {
    if (!elsewhere) localStorage.setItem(PAGE, location);
  }, [elsewhere, location]);
  useEscape(() => (batch.on ? batch.end() : void hidePanel()));
  if (elsewhere) return null;
  if (folder && !board.folder(folder)) return <Redirect to="/" replace />;
  return (
    <>
      <header className="panel-head">
        {batch.on ? (
          <BatchHead />
        ) : (
          <>
            <Pages folder={next ? undefined : folder} />
            <span className="hero-actions">
              {next && <NextFilter />}
              <Tip label="在主窗口打开">
                <button className="icon-btn" onClick={() => void openMain(location)}>
                  <Icon of={AppWindow} />
                </button>
              </Tip>
            </span>
          </>
        )}
      </header>
      <ConnectionNote connection={connection} />
      {!batch.on && <QuickAdd key={location} folder={next ? undefined : folder} />}
      <div className="panel-scroll">{next ? <NextBody now={now} /> : <FolderBody folder={folder} now={now} />}</div>
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

/** Says which page it shows, `undefined` for what is next, and switches to another: what is next, or
 * any folder, those others share in a group after the user's own. */
function Pages({ folder }: { folder?: FolderId | null }) {
  const { board } = useView();
  const [, navigate] = useLocation();
  const root = useContext(RootContext);
  const item = (key: string, to: string, icon: ReactNode, name: ReactNode, on: boolean, depth = 0) => (
    <DropdownMenu.Item key={key} className="menu-item" style={{ paddingLeft: 12 + depth * 16 }} onSelect={() => navigate(to)}>
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
    item(id ?? "root", folderUrl(id), id ? <FolderGlyph board={board} id={id} /> : <Icon of={FolderRoot} />, <FolderName board={board} id={id} />, folder === id, depth),
    ...board.subfolders(id).flatMap((f) => tree(f.id, depth + 1)),
  ];
  const theirs = board.sharedWithMe();
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="panel-pages">
        <span className="truncate">{folder === undefined ? "接下来" : <FolderName board={board} id={folder} />}</span>
        <Icon of={ChevronDown} size={18} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal container={root}>
        <DropdownMenu.Content className="menu panel-menu" align="start" sideOffset={4}>
          {item("next", "/", <Icon of={CircleCheck} />, "接下来", folder === undefined)}
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

/** Signed out, or local mode would not start: the main window is where to see to it, and a sign-in
 * there loads the panel again (main.tsx). */
export function PanelOut({ failed }: { failed?: string }) {
  useEscape(() => void hidePanel());
  return (
    <div className="panel-out">
      {failed && <p>{failed}</p>}
      <button className="filled-btn" onClick={() => void openMain("/")}>
        {failed ? "打开 NextTodo" : "打开 NextTodo 登录"}
      </button>
    </div>
  );
}
