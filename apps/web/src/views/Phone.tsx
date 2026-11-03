// The phone layout: one page at a time with a tab bar, details in a bottom sheet.

import { ChevronLeft, CircleCheck, Folder as FolderIcon, History, Search } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Redirect, useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { formatToday } from "../domain/dates";
import { useConnection, useSession, useView } from "../session";
import { useBack } from "../ui/back";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { Sheet } from "../ui/Sheet";
import { app } from "../api/http";
import { BackgroundTip } from "./Background";
import { Detail } from "./Detail";
import { FolderMenu } from "./FolderMenu";
import { DoneBody, FolderBody, FolderGraph, FolderViewSeg, NextBody, QuickAdd, SearchResults } from "./lists";
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

  // Going deeper in a tab (into a folder, to what is done) slides the new page in from the right;
  // going back, the other way.
  const depth = tab === "next" ? Number(location === "/done") : folder && board.folder(folder) ? board.depth(folder) : 0;
  const [page, setPage] = useState({ tab, depth, anim: "" });
  if (page.tab !== tab || page.depth !== depth) {
    const anim = tab === page.tab ? (depth > page.depth ? " anim-push" : " anim-pop") : "";
    setPage({ tab, depth, anim });
  }
  const anim = page.anim;

  let screen: ReactNode;
  if (location === "/done") screen = <DoneScreen key="done" anim={anim} now={now} />;
  else if (tab === "next") screen = <NextScreen key="next" anim={anim} now={now} />;
  else if (folder && !board.folder(folder)) screen = <Redirect to="/folders" replace />;
  else screen = <FolderScreen key={folder ?? "root"} anim={anim} folder={folder} now={now} />;

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
      <header className="nav" data-tauri-drag-region="deep">
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

/** The top bar's way back to the page above, which Android's back button takes too. */
function Back({ to, label }: { to: string; label: string }) {
  const [, navigate] = useLocation();
  const up = () => navigate(to);
  useBack(up);
  return (
    <button className="back" onClick={up}>
      <Icon of={ChevronLeft} size={22} />
      <span>{label}</span>
    </button>
  );
}

function NextScreen({ anim, now }: { anim: string; now: Date }) {
  const { board } = useView();
  const connection = useConnection();
  const [, navigate] = useLocation();
  return (
    <Screen title="接下来" anim={anim} after={<QuickAdd />}>
      <div className="hero">
        <h1 className="large-title">接下来</h1>
        <button className="icon-btn" onClick={() => navigate("/done")} aria-label="已完成" title="已完成">
          <Icon of={History} />
        </button>
      </div>
      <p className="subtitle">
        {formatToday(now)}，{board.next().length} 项可做
      </p>
      <ConnectionNote connection={connection} />
      {app === "android" && <BackgroundTip />}
      <NextBody now={now} />
    </Screen>
  );
}

function DoneScreen({ anim, now }: { anim: string; now: Date }) {
  return (
    <Screen title="已完成" nav={<Back to="/" label="接下来" />} anim={anim}>
      <div className="hero">
        <h1 className="large-title">已完成</h1>
      </div>
      <DoneBody now={now} />
    </Screen>
  );
}

/** A folder, or the root (`null`). The root is where the folders are reached from, so it is always a
 * list, with the search over all tasks. */
function FolderScreen({ folder, anim, now }: { folder: FolderId | null; anim: string; now: Date }) {
  const { board } = useView();
  const [view] = useFolderView();
  const [query, setQuery] = useState("");
  const f = folder ? board.folder(folder)! : null;
  const title = f ? f.name : "文件夹";
  const nav = f && <Back to={folderUrl(f.parent_id)} label={f.parent_id ? board.folderName(f.parent_id) : "文件夹"} />;
  const menu = folder && <FolderMenu folder={folder} />;
  if (f && view === "graph") {
    return (
      <section className={`screen scrolled${anim}`}>
        <header className="nav" data-tauri-drag-region="deep">
          {nav ?? <span />}
          <span className="nav-title">{title}</span>
          <span className="nav-actions">{menu}</span>
        </header>
        <div className="nav-space" />
        <div className="view-seg">
          <FolderViewSeg />
        </div>
        <FolderGraph folder={f.id} across={false} now={now} />
      </section>
    );
  }
  return (
    <Screen title={title} nav={nav} anim={anim} after={<QuickAdd folder={folder} />}>
      <div className="hero">
        <h1 className="large-title">{title}</h1>
        {menu}
      </div>
      {f ? (
        <div className="view-seg">
          <FolderViewSeg />
        </div>
      ) : (
        <label className="search" style={{ marginTop: 12 }}>
          <Icon of={Search} size={18} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索全部事项" autoComplete="off" />
        </label>
      )}
      {query.trim() ? <SearchResults query={query} now={now} /> : <FolderBody folder={folder} now={now} />}
    </Screen>
  );
}
