// The desktop layout: folders on the left, which can be folded away, the list in the middle, and the
// open task's details on the right.

import { ChevronRight, CircleCheck, Folder as FolderIcon, FolderRoot, History, PanelLeft, Search } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Redirect, useLocation, useSearchParams } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { formatToday } from "../domain/dates";
import { useConnection, useSession, useView } from "../session";
import { useBack } from "../ui/back";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { Detail } from "./Detail";
import { FolderMenu } from "./FolderMenu";
import { DoneBody, FolderBody, FolderGraph, FolderViewSeg, NextBody, QuickAdd, SearchResults } from "./lists";
import { folderUrl, useFolderView, useSelection } from "./nav";

export function Desktop({ onSettings }: { onSettings(): void }) {
  const [location] = useLocation();
  const { board } = useView();
  const [sideOpen, setSideOpen] = useState(true);
  const { selected, close } = useSelection();
  // As on phones, a task not here (yet) shows nothing: one opened from outside may still be on its way.
  const open = selected !== null && board.task(selected) !== undefined;
  useBack("panel", open ? close : null);
  // The pane keeps showing the task it was closed on while it slides away, and lets go of it after.
  const [shown, setShown] = useState(open ? selected : null);
  if (open && selected !== shown) setShown(selected);
  const now = useNow();
  const [query, setQuery] = useState("");
  // Quick add (`/?add`) is on the page of what to do next, which a search covers.
  const [params] = useSearchParams();
  if (params.has("add") && query) setQuery("");
  const [view] = useFolderView();
  const folder = location.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
  const at = location === "/done" ? "done" : location.startsWith("/folders") ? (folder ?? "root") : "next";

  let main: ReactNode;
  if (query.trim()) {
    main = (
      <Page title="搜索">
        <SearchResults query={query} now={now} />
      </Page>
    );
  } else if (at === "next") {
    main = (
      <Page title="接下来" crumbs={formatToday(now)} add={<QuickAdd key="next" />}>
        <NextBody now={now} />
      </Page>
    );
  } else if (at === "done") {
    main = (
      <Page title="已完成">
        <DoneBody now={now} />
      </Page>
    );
  } else if (at !== "root" && !board.folder(at)) {
    main = <Redirect to="/folders" replace />;
  } else {
    const f = at === "root" ? null : board.folder(at)!;
    const id = f?.id ?? null;
    const title = board.folderName(id);
    const actions = (
      <div className="head-actions">
        <FolderViewSeg />
        {f && <FolderMenu folder={f.id} />}
      </div>
    );
    const crumbs = f ? board.place(f.parent_id) : "";
    main =
      view === "graph" ? (
        <Page title={title} crumbs={crumbs} actions={actions} scroll={false}>
          <FolderGraph folder={id} across now={now} />
        </Page>
      ) : (
        <Page title={title} crumbs={crumbs} actions={actions} add={<QuickAdd key={at} folder={id} />}>
          <FolderBody key={at} folder={id} now={now} />
        </Page>
      );
  }

  const label = `${sideOpen ? "收起" : "展开"}左侧栏`;
  return (
    <div
      className={`desk-cols${sideOpen ? "" : " side-closed"}${open ? "" : " pane-closed"}`}
      onTransitionEnd={(e) => e.target === e.currentTarget && !open && setShown(null)}
    >
      <Sidebar at={query.trim() ? null : at} query={query} setQuery={setQuery} onSettings={onSettings} />
      <main className="main">
        <div className="main-tools" data-tauri-drag-region="deep">
          <button className="icon-btn" onClick={() => setSideOpen(!sideOpen)} aria-pressed={sideOpen} aria-label={label} title={label}>
            <Icon of={PanelLeft} />
          </button>
        </div>
        {main}
      </main>
      <aside className="detail-pane" inert={!open}>
        <div className="pane-inner">{shown && <Detail key={shown} id={shown} now={now} onClose={close} />}</div>
      </aside>
    </div>
  );
}

function Page({ title, crumbs = "", actions, add, scroll = true, children }: { title: string; crumbs?: string; actions?: ReactNode; add?: ReactNode; scroll?: boolean; children: ReactNode }) {
  return (
    <>
      <header className="main-head" data-tauri-drag-region="deep">
        <div>
          <div className="crumbs">{crumbs}</div>
          <h1 className="main-title">{title}</h1>
        </div>
        {actions}
      </header>
      {add}
      {scroll ? <div className="main-scroll">{children}</div> : children}
    </>
  );
}

function Sidebar({ at, query, setQuery, onSettings }: { at: FolderId | "next" | "done" | "root" | null; query: string; setQuery(q: string): void; onSettings(): void }) {
  const { board, pending } = useView();
  const { user } = useSession();
  const connection = useConnection();
  const [, navigate] = useLocation();
  const [collapsed, setCollapsed] = useState(() => new Set<FolderId | null>());
  const go = (to: string) => {
    setQuery("");
    navigate(to);
  };
  const toggle = (id: FolderId | null) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  // The root first, with the folders inside it.
  const tree = (id: FolderId | null, depth: number): ReactNode => {
    const kids = board.subfolders(id);
    const open = !collapsed.has(id);
    const count = board.contents(id).filter((t) => t.done_at === null).length;
    return (
      <div key={id ?? "root"}>
        <button className={`sb-item${at === (id ?? "root") ? " on" : ""}`} style={{ paddingLeft: 10 + depth * 18 }} onClick={() => go(folderUrl(id))}>
          <span
            className={`tw${kids.length && open ? " open" : ""}`}
            onClick={(e) => {
              if (!kids.length) return;
              e.stopPropagation();
              toggle(id);
            }}
          >
            {kids.length > 0 && <Icon of={ChevronRight} size={14} width={2.2} />}
          </span>
          <Icon of={id ? FolderIcon : FolderRoot} size={18} />
          <span className="truncate">{board.folderName(id)}</span>
          <span className="count">{count || ""}</span>
        </button>
        {open && kids.map((f) => tree(f.id, depth + 1))}
      </div>
    );
  };
  return (
    <nav className="sidebar">
      <div className="sb-inner">
        <div className="sb-head" data-tauri-drag-region="deep">
          <span className="brand">
            <span className="brand-mark">
              <Icon of={CircleCheck} size={18} width={2} />
            </span>
            NextTodo
          </span>
        </div>
        <label className="search">
          <Icon of={Search} size={18} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索" autoComplete="off" />
        </label>
        <div className="sb-scroll">
          <button className={`sb-item${at === "next" ? " on" : ""}`} onClick={() => go("/")}>
            <span className="tw" />
            <Icon of={CircleCheck} size={18} />
            <span>接下来</span>
            <span className="count">{board.next().length}</span>
          </button>
          <button className={`sb-item${at === "done" ? " on" : ""}`} onClick={() => go("/done")}>
            <span className="tw" />
            <Icon of={History} size={18} />
            <span>已完成</span>
          </button>
          <div className="sb-label">文件夹</div>
          {tree(null, 0)}
        </div>
        <div className="sb-foot">
          <button className="sb-user" onClick={onSettings}>
            <span className="avatar">{[...user.display_name][0]}</span>
            <span className="folder-main">
              <span>{user.display_name}</span>
              <span className="folder-sub">{connection === "offline" ? (pending ? `离线，${pending} 项改动待同步` : "离线") : "设置和令牌"}</span>
            </span>
          </button>
        </div>
      </div>
    </nav>
  );
}
