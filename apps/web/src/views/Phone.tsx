// The phone layout: one page at a time with a tab bar, details in a bottom sheet.

import { ChevronLeft, CircleCheck, Folder as FolderIcon, Search } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Redirect, useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { formatToday } from "../domain/dates";
import { useConnection, useSession, useView } from "../session";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { Sheet } from "../ui/Sheet";
import { native } from "../api/http";
import { BackgroundTip } from "./Background";
import { Detail } from "./Detail";
import { FolderMenu } from "./FolderMenu";
import { FolderBody, FolderGraph, FolderViewSeg, NextBody, QuickAdd, RootBody, SearchResults } from "./lists";
import { folderUrl, useFolderView, useSelection } from "./nav";
import { ConnectionNote } from "./status";

export function Phone({ onSettings }: { onSettings(): void }) {
  const [location, navigate] = useLocation();
  const { board } = useView();
  const { user } = useSession();
  const { selected, close } = useSelection();
  const now = useNow();
  const folder = location.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
  const tab = location.startsWith("/folders") ? "folders" : "next";

  // Deeper into the folders slides the new page in from the right; going back, the other way.
  const depth = folder && board.folder(folder) ? 1 + board.depth(folder) : 1;
  const [page, setPage] = useState({ tab, depth, anim: "" });
  if (page.tab !== tab || page.depth !== depth) {
    const anim = tab === "folders" && page.tab === "folders" ? (depth > page.depth ? " anim-push" : " anim-pop") : "";
    setPage({ tab, depth, anim });
  }
  const anim = page.anim;

  let screen: ReactNode;
  if (tab === "next") screen = <NextScreen now={now} />;
  else if (!folder) screen = <FoldersScreen key="root" anim={anim} now={now} />;
  else if (!board.folder(folder)) screen = <Redirect to="/folders" replace />;
  else screen = <FolderScreen key={folder} anim={anim} folder={folder} now={now} />;

  const tabButton = (id: string, label: string, icon: typeof FolderIcon, to: string) => (
    <button className={`tab${tab === id ? " on" : ""}`} onClick={() => navigate(to)}>
      <span className="tab-ic">
        <Icon of={icon} size={22} />
      </span>
      {label}
    </button>
  );
  return (
    <>
      {screen}
      <nav className="tabbar">
        {tabButton("next", "接下来", CircleCheck, "/")}
        {tabButton("folders", "文件夹", FolderIcon, "/folders")}
        <button className="tab" onClick={onSettings}>
          <span className="tab-ic">
            <span className="avatar">{[...user.display_name][0]}</span>
          </span>
          设置
        </button>
      </nav>
      {selected && board.task(selected) && (
        <Sheet label="事项详情" onClose={close}>
          <Detail id={selected} now={now} onClose={close} />
        </Sheet>
      )}
    </>
  );
}

/** A page with a large title that gives way to a small one in a frosted bar once scrolled past. */
function Screen({ title, nav, anim = "", children, after }: { title: string; nav?: ReactNode; anim?: string; children: ReactNode; after?: ReactNode }) {
  const [scrolled, setScrolled] = useState(false);
  return (
    <section className={`screen${scrolled ? " scrolled" : ""}${anim}`}>
      <header className="nav">
        {nav ?? <span />}
        <span className="nav-title">{title}</span>
        <span />
      </header>
      <div className="scroll" onScroll={(e) => setScrolled(e.currentTarget.scrollTop > 36)}>
        {children}
      </div>
      {after}
    </section>
  );
}

function NextScreen({ now }: { now: Date }) {
  const { board } = useView();
  const connection = useConnection();
  return (
    <Screen title="接下来" after={<QuickAdd />}>
      <div className="hero">
        <h1 className="large-title">接下来</h1>
      </div>
      <p className="subtitle">
        {formatToday(now)}，{board.next().length} 项可做
      </p>
      <ConnectionNote connection={connection} />
      {native && <BackgroundTip />}
      <NextBody now={now} />
    </Screen>
  );
}

function FoldersScreen({ anim, now }: { anim: string; now: Date }) {
  const [query, setQuery] = useState("");
  return (
    <Screen title="文件夹" anim={anim}>
      <div className="hero">
        <h1 className="large-title">文件夹</h1>
      </div>
      <label className="search" style={{ marginTop: 12 }}>
        <Icon of={Search} size={18} />
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索全部事项" autoComplete="off" />
      </label>
      {query.trim() ? <SearchResults query={query} now={now} /> : <RootBody now={now} />}
    </Screen>
  );
}

function FolderScreen({ folder, anim, now }: { folder: FolderId; anim: string; now: Date }) {
  const { board } = useView();
  const [, navigate] = useLocation();
  const [view] = useFolderView();
  const f = board.folder(folder)!;
  const parent = f.parent_id ? board.folder(f.parent_id)!.name : "文件夹";
  const nav = (
    <button className="back" onClick={() => navigate(folderUrl(f.parent_id))}>
      <Icon of={ChevronLeft} size={22} />
      <span>{parent}</span>
    </button>
  );
  if (view === "graph") {
    return (
      <section className={`screen scrolled${anim}`}>
        <header className="nav">
          {nav}
          <span className="nav-title">{f.name}</span>
          <span className="nav-actions">
            <FolderMenu folder={folder} />
          </span>
        </header>
        <div className="nav-space" />
        <div className="view-seg">
          <FolderViewSeg />
        </div>
        <FolderGraph folder={folder} across={false} now={now} />
      </section>
    );
  }
  return (
    <Screen title={f.name} nav={nav} anim={anim} after={<QuickAdd folder={folder} />}>
      <div className="hero">
        <h1 className="large-title">{f.name}</h1>
        <FolderMenu folder={folder} />
      </div>
      <div className="view-seg">
        <FolderViewSeg />
      </div>
      <FolderBody folder={folder} now={now} />
    </Screen>
  );
}
