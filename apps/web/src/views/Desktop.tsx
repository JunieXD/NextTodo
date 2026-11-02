// The desktop layout: folders on the left, the list in the middle, details on the right. Both sides
// can be folded away.

import { ChevronRight, CircleCheck, Folder as FolderIcon, PanelLeft, PanelRight, Search } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Redirect, useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { formatToday } from "../domain/dates";
import { useConnection, useSession, useView } from "../session";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { Detail } from "./Detail";
import { FolderMenu } from "./FolderMenu";
import { FolderBody, NextBody, QuickAdd, RootBody, SearchResults } from "./lists";
import { folderUrl, useSelection } from "./nav";
import { place } from "./rows";

export function Desktop({ sideOpen, paneOpen, toggleSide, togglePane, onSettings }: { sideOpen: boolean; paneOpen: boolean; toggleSide(): void; togglePane(): void; onSettings(): void }) {
  const [location] = useLocation();
  const { board } = useView();
  const { selected } = useSelection();
  const now = useNow();
  const [query, setQuery] = useState("");
  const folder = location.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
  const at = location.startsWith("/folders") ? (folder ?? "root") : "next";

  let main: ReactNode;
  if (query.trim()) {
    main = (
      <Page title="搜索">
        <SearchResults query={query} now={now} />
      </Page>
    );
  } else if (at === "next") {
    main = (
      <Page title="接下来" crumbs={formatToday(now)} add={<QuickAdd />}>
        <NextBody now={now} />
      </Page>
    );
  } else if (at === "root") {
    main = (
      <Page title="文件夹">
        <RootBody now={now} />
      </Page>
    );
  } else if (!board.folder(at)) {
    main = <Redirect to="/folders" replace />;
  } else {
    const f = board.folder(at)!;
    main = (
      <Page title={f.name} crumbs={f.parent_id ? place(board, f.parent_id) : ""} actions={<FolderMenu folder={f.id} />} add={<QuickAdd folder={f.id} />}>
        <FolderBody key={f.id} folder={f.id} now={now} />
      </Page>
    );
  }

  const tool = (onClick: () => void, icon: typeof PanelLeft, open: boolean, label: string) => (
    <button className="icon-btn" onClick={onClick} aria-pressed={open} aria-label={`${open ? "收起" : "展开"}${label}`} title={`${open ? "收起" : "展开"}${label}`}>
      <Icon of={icon} />
    </button>
  );
  return (
    <>
      <Sidebar at={query.trim() ? null : at} query={query} setQuery={setQuery} onSettings={onSettings} />
      <main className="main">
        <div className="main-tools">
          {tool(toggleSide, PanelLeft, sideOpen, "左侧栏")}
          {tool(togglePane, PanelRight, paneOpen, "详情栏")}
        </div>
        {main}
      </main>
      <aside className="detail-pane">
        <div className="pane-inner">{selected ? <Detail key={selected} id={selected} now={now} /> : <div className="detail-empty">选一个事项，在这里查看和编辑</div>}</div>
      </aside>
    </>
  );
}

function Page({ title, crumbs = "", actions, add, children }: { title: string; crumbs?: string; actions?: ReactNode; add?: ReactNode; children: ReactNode }) {
  return (
    <>
      <header className="main-head">
        <div>
          <div className="crumbs">{crumbs}</div>
          <h1 className="main-title">{title}</h1>
        </div>
        {actions}
      </header>
      {add}
      <div className="main-scroll">{children}</div>
    </>
  );
}

function Sidebar({ at, query, setQuery, onSettings }: { at: FolderId | "next" | "root" | null; query: string; setQuery(q: string): void; onSettings(): void }) {
  const { board, pending } = useView();
  const { user } = useSession();
  const connection = useConnection();
  const [, navigate] = useLocation();
  const [collapsed, setCollapsed] = useState(() => new Set<FolderId>());
  const go = (to: string) => {
    setQuery("");
    navigate(to);
  };
  const toggle = (id: FolderId) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const tree = (parent: FolderId | null, depth: number): ReactNode =>
    board.subfolders(parent).map((f) => {
      const kids = board.subfolders(f.id).length > 0;
      const open = !collapsed.has(f.id);
      const count = board.contents(f.id).filter((t) => t.done_at === null).length;
      return (
        <div key={f.id}>
          <button className={`sb-item${at === f.id ? " on" : ""}`} style={{ paddingLeft: 10 + depth * 18 }} onClick={() => go(folderUrl(f.id))}>
            <span
              className={`tw${kids && open ? " open" : ""}`}
              onClick={(e) => {
                if (!kids) return;
                e.stopPropagation();
                toggle(f.id);
              }}
            >
              {kids && <Icon of={ChevronRight} size={14} width={2.2} />}
            </span>
            <Icon of={FolderIcon} size={18} />
            <span className="truncate">{f.name}</span>
            <span className="count">{count || ""}</span>
          </button>
          {kids && open && tree(f.id, depth + 1)}
        </div>
      );
    });
  return (
    <nav className="sidebar">
      <div className="sb-inner">
        <div className="sb-head">
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
          <div className="sb-label">
            <button onClick={() => go("/folders")}>文件夹</button>
          </div>
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
