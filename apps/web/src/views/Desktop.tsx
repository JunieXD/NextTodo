// The desktop layout: folders on the left, which can be folded away, the list in the middle, and the
// open task's details on the right.

import { CalendarDays, ChevronLeft, ChevronRight, CircleCheck, FolderRoot, History, ListChecks, Lock, PanelLeft, Search } from "lucide-react";
import { type CSSProperties, Fragment, type ReactNode, useRef, useState } from "react";
import { Link, Redirect, useLocation, useSearchParams } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { formatToday } from "../domain/dates";
import { lockNow, usePrivacyLock } from "../lock";
import { useConnection, useSession, useView } from "../session";
import { useBack } from "../ui/back";
import { Tip } from "../ui/feedback";
import { Fold } from "../ui/fold";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { BatchTools, batchTitle, useBatch } from "./batch";
import { useColumns } from "./columns";
import { Detail } from "./Detail";
import { useTreeDrag } from "./drag";
import { FolderContextMenu, FolderMenu, usePlaceFolder } from "./FolderMenu";
import { CalendarBody, CalendarViewSeg } from "./calendar";
import { setFolderClosed, useClosedFolders } from "./closed-folders";
import { DoneBody, FolderBody, FolderGraph, FolderViewSeg, NextBody, NextFilter, QuickAdd, SearchResults } from "./lists";
import { readyIn, useNextFolders } from "../next-folders";
import { folderUrl, settingsUrl, useFolderView, useSelection } from "./nav";
import { SettingsHome, SettingsPage, settingsTitle, useSectionAt } from "./Settings";
import { FolderGlyph, FolderTile, styled } from "./folder-look";
import { FolderName, SharedWith, sharer } from "./share";

export function Desktop({ onSignOut }: { onSignOut(): void }) {
  const [location] = useLocation();
  const { board } = useView();
  const [sideOpen, setSideOpen] = useState(true);
  const { selected, close } = useSelection();
  // As on phones, a task not here (yet) shows nothing: one opened from outside may still be on its way.
  const open = selected !== null && board.task(selected) !== undefined;
  const columns = useColumns({ sidebar: sideOpen, pane: open });
  useBack("panel", open ? close : null);
  // The pane keeps showing the task it was closed on while it slides away, and lets go of it after.
  const [shown, setShown] = useState(open ? selected : null);
  if (open && selected !== shown) setShown(selected);
  const now = useNow();
  const batch = useBatch();
  const [query, setQuery] = useState("");
  // Searching leaves the page the tasks were being chosen on.
  const search = (q: string) => {
    setQuery(q);
    batch.end();
  };
  // Quick add (`/?add`) is on the page of what to do next, which a search covers.
  const [params] = useSearchParams();
  if (params.has("add") && query) setQuery("");
  const [view] = useFolderView();
  const folder = location.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
  const section = useSectionAt(location);
  const at =
    location === "/done"
      ? "done"
      : location === "/calendar"
        ? "calendar"
        : location.startsWith("/folders")
          ? (folder ?? "root")
          : location.startsWith("/settings")
            ? "settings"
            : "next";

  let main: ReactNode;
  if (query.trim()) {
    main = (
      <Page title="搜索">
        <SearchResults query={query} now={now} />
      </Page>
    );
  } else if (at === "next") {
    main = (
      <Page
        title="接下来"
        crumbs={formatToday(now)}
        actions={
          <div className="head-actions">
            <NextFilter />
          </div>
        }
        add={<QuickAdd key="next" />}
      >
        <NextBody now={now} />
      </Page>
    );
  } else if (at === "settings") {
    main =
      section === undefined ? (
        <Redirect to={settingsUrl()} replace />
      ) : (
        <Page
          title={section ? settingsTitle(section) : "设置"}
          crumbs={
            section && (
              <Link className="crumb-back" to={settingsUrl()}>
                <Icon of={ChevronLeft} size={16} />
                设置
              </Link>
            )
          }
          narrow
        >
          {section ? <SettingsPage section={section} onSignOut={onSignOut} /> : <SettingsHome />}
        </Page>
      );
  } else if (at === "done") {
    const any = board.snapshot.tasks.some((t) => t.done_at !== null && board.mine(t));
    const actions = batch.on ? (
      <BatchTools />
    ) : (
      any && (
        <div className="head-actions">
          <Tip label="批量操作">
            <button className="icon-btn" onClick={() => batch.start()}>
              <Icon of={ListChecks} />
            </button>
          </Tip>
        </div>
      )
    );
    main = (
      <Page title={batch.on ? batchTitle(batch.tasks.length) : "已完成"} actions={actions}>
        <DoneBody now={now} />
      </Page>
    );
  } else if (at === "calendar") {
    main = (
      <Page
        title="日历"
        actions={
          <div className="head-actions">
            <CalendarViewSeg />
          </div>
        }
        narrow
      >
        <CalendarBody now={now} />
      </Page>
    );
  } else if (at !== "root" && !board.folder(at)) {
    main = <Redirect to="/folders" replace />;
  } else {
    const f = at === "root" ? null : board.folder(at)!;
    const id = f?.id ?? null;
    const title = batch.on ? batchTitle(batch.tasks.length) : board.folderName(id);
    const actions = batch.on ? (
      <BatchTools />
    ) : (
      <div className="head-actions">
        <SharedWith folder={id} />
        <FolderViewSeg />
        <FolderMenu folder={id} />
      </div>
    );
    const by = f && sharer(board, f.id);
    const crumbs = by ? `${board.name(by)} 共享` : f ? board.place(f.parent_id) : "";
    const icon = !batch.on && styled(board, id) && <FolderTile board={board} id={id} className="title-ic" />;
    main =
      view === "graph" ? (
        <Page title={title} icon={icon} crumbs={crumbs} actions={actions} scroll={false}>
          <FolderGraph folder={id} across now={now} />
        </Page>
      ) : (
        <Page title={title} icon={icon} crumbs={crumbs} actions={actions} add={!batch.on && <QuickAdd key={at} folder={id} />}>
          <FolderBody key={at} folder={id} now={now} />
        </Page>
      );
  }

  const label = `${sideOpen ? "收起" : "展开"}左侧栏`;
  return (
    <div
      className={`desk-cols${sideOpen ? "" : " side-closed"}${open ? "" : " pane-closed"}${columns.dragging ? " resizing" : ""}`}
      style={{ "--sidebar": `${columns.shown.sidebar}px`, "--pane": `${columns.shown.pane}px` } as CSSProperties}
      onTransitionEnd={(e) => e.target === e.currentTarget && !open && setShown(null)}
    >
      <Sidebar at={query.trim() ? null : at} query={query} setQuery={search} />
      <main className={`main${batch.on ? " choosing" : ""}`}>
        {sideOpen && <div className="col-edge sidebar-edge" {...columns.edge("sidebar")} />}
        {open && <div className="col-edge pane-edge" {...columns.edge("pane")} />}
        <div className="main-tools" data-tauri-drag-region="deep">
          <Tip label={label}>
            <button className="icon-btn" onClick={() => setSideOpen(!sideOpen)} aria-pressed={sideOpen}>
              <Icon of={PanelLeft} />
            </button>
          </Tip>
        </div>
        {/* Each place a page of its own, which fades in. */}
        <Fragment key={query.trim() ? "search" : `${at} ${section} ${view}`}>{main}</Fragment>
      </main>
      <aside className="detail-pane" inert={!open}>
        <div className="pane-inner">{shown && <Detail key={shown} id={shown} now={now} closing={!open} onClose={close} />}</div>
      </aside>
    </div>
  );
}

/** A page in the middle column; `narrow`, its title above the one column of the settings. */
function Page(p: {
  title: string;
  /** Before the title: a folder's, when it has a colour or icon chosen. */
  icon?: ReactNode;
  crumbs?: ReactNode;
  actions?: ReactNode;
  add?: ReactNode;
  scroll?: boolean;
  narrow?: boolean;
  children: ReactNode;
}) {
  const { title, icon, crumbs, actions, add, scroll = true, children } = p;
  return (
    <>
      <header className={`main-head${p.narrow ? " narrow" : ""}`} data-tauri-drag-region="deep">
        <div>
          <div className="crumbs">{crumbs}</div>
          <h1 className="main-title">
            {icon}
            {title}
          </h1>
        </div>
        {actions}
      </header>
      {add}
      {scroll ? <div className="main-scroll">{children}</div> : children}
    </>
  );
}

function Sidebar({ at, query, setQuery }: { at: FolderId | "next" | "done" | "calendar" | "root" | "settings" | null; query: string; setQuery(q: string): void }) {
  const { board, pending } = useView();
  const listed = useNextFolders(board);
  const { user } = useSession();
  const connection = useConnection();
  const lock = usePrivacyLock();
  const [, navigate] = useLocation();
  const collapsed = useClosedFolders(board.me);
  const go = (to: string) => {
    setQuery("");
    navigate(to);
  };
  const scroll = useRef<HTMLDivElement>(null);
  const place = usePlaceFolder();
  useTreeDrag(scroll, {
    canPlace: (folder, parent) => board.canMove(folder, parent),
    place,
    unfold: (id) => setFolderClosed(board.me, id, false),
  });
  // The root first, with the folders inside it; then each folder others share with the user, which
  // stays where it is, with who shares it.
  const tree = (id: FolderId | null, parent: FolderId | null, depth: number): ReactNode => {
    const kids = board.subfolders(id);
    const open = !collapsed.has(id);
    const count = board.within(id).filter((t) => t.done_at === null).length;
    const theirs = id && sharer(board, id);
    return (
      <div key={id ?? "root"}>
        <FolderContextMenu folder={id}>
          <button
            className={`sb-item${at === (id ?? "root") ? " on" : ""}`}
            style={{ paddingLeft: 10 + depth * 18 }}
            data-into={id ?? ""}
            data-parent={parent ?? ""}
            data-depth={depth}
            data-fixed={theirs ? "" : undefined}
            data-closed={kids.length && !open ? "" : undefined}
            onClick={() => go(folderUrl(id))}
          >
            <span
              className={`tw${kids.length && open ? " open" : ""}`}
              onClick={(e) => {
                if (!kids.length) return;
                e.stopPropagation();
                setFolderClosed(board.me, id, open);
              }}
            >
              {kids.length > 0 && <Icon of={ChevronRight} size={14} width={2.2} />}
            </span>
            {id ? <FolderGlyph board={board} id={id} /> : <Icon of={FolderRoot} size={18} />}
            <span className="truncate">
              <FolderName board={board} id={id} />
            </span>
            <span className="count">{count || ""}</span>
          </button>
        </FolderContextMenu>
        {kids.length > 0 && <Fold open={open}>{kids.map((f) => tree(f.id, id, depth + 1))}</Fold>}
      </div>
    );
  };
  return (
    <nav className="sidebar">
      <div className="sb-inner">
        <div className="sb-head" data-tauri-drag-region="deep">
          <span className="brand">
            <img className="brand-mark" src="/icon.svg" alt="" />
            NextTodo
          </span>
        </div>
        <label className="search">
          <Icon of={Search} size={18} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索" autoComplete="off" />
        </label>
        <div className="sb-scroll" ref={scroll}>
          <button className={`sb-item${at === "next" ? " on" : ""}`} onClick={() => go("/")}>
            <span className="tw" />
            <Icon of={CircleCheck} size={18} />
            <span>接下来</span>
            <span className="count">{readyIn(board, listed).length}</span>
          </button>
          <button className={`sb-item${at === "done" ? " on" : ""}`} onClick={() => go("/done")}>
            <span className="tw" />
            <Icon of={History} size={18} />
            <span>已完成</span>
          </button>
          <button className={`sb-item${at === "calendar" ? " on" : ""}`} onClick={() => go("/calendar")}>
            <span className="tw" />
            <Icon of={CalendarDays} size={18} />
            <span>日历</span>
          </button>
          <div className="sb-label">文件夹</div>
          {tree(null, null, 0)}
          {board.sharedWithMe().length > 0 && (
            <div data-theirs>
              <div className="sb-label">共享给我</div>
              {board.sharedWithMe().map((f) => tree(f.id, null, 0))}
            </div>
          )}
        </div>
        <div className="sb-foot">
          <button className={`sb-user${at === "settings" ? " on" : ""}`} onClick={() => go(settingsUrl())}>
            <span className="avatar">{[...user.display_name][0]}</span>
            <span className="folder-main">
              <span>{user.display_name}</span>
              <span className="folder-sub">{connection === "offline" ? (pending ? `离线，${pending} 项改动待同步` : "离线") : "设置和令牌"}</span>
            </span>
          </button>
          {lock && (
            <Tip label="锁定">
              <button className="icon-btn" onClick={lockNow}>
                <Icon of={Lock} size={18} />
              </button>
            </Tip>
          )}
        </div>
      </div>
    </nav>
  );
}
