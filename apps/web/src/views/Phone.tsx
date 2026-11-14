// The phone layout: one page at a time with a tab bar, details in a bottom sheet.

import { ChevronLeft, CircleCheck, Folder as FolderIcon, History, ListChecks, Search } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Redirect, useLocation } from "wouter";
import type { FolderId } from "../api/types/FolderId";
import { formatToday } from "../domain/dates";
import { useConnection, useSession, useView } from "../session";
import { useBack } from "../ui/back";
import { Tip } from "../ui/feedback";
import { useNow } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { Sheet } from "../ui/Sheet";
import { app } from "../api/http";
import { BackgroundTip } from "./Background";
import { BatchBar, BatchHead, useBatch } from "./batch";
import { Detail } from "./Detail";
import { FolderMenu } from "./FolderMenu";
import { DoneBody, FolderBody, FolderGraph, FolderViewSeg, NextBody, QuickAdd, SearchResults } from "./lists";
import { folderUrl, type SettingsSection, settingsUrl, useFolderView, useSelection } from "./nav";
import { SettingsHome, SettingsPage, settingsTitle, useSectionAt } from "./Settings";
import { ConnectionNote } from "./status";

export function Phone({ onSignOut }: { onSignOut(): void }) {
  const [location, navigate] = useLocation();
  const { board } = useView();
  const { user } = useSession();
  const { selected, close } = useSelection();
  const batch = useBatch();
  const now = useNow();
  const folder = location.match(/^\/folders\/([^/]+)$/)?.[1] ?? null;
  const section = useSectionAt(location);
  const tab = location.startsWith("/folders") ? "folders" : location.startsWith("/settings") ? "settings" : "next";

  // Going deeper in a tab (into a folder, to what is done, to a section of the settings) slides the
  // new page in from the right; going back, the other way.
  const depth =
    tab === "next" ? Number(location === "/done") : tab === "settings" ? Number(!!section) : folder && board.folder(folder) ? board.depth(folder) : 0;
  const [page, setPage] = useState({ tab, depth, anim: "" });
  if (page.tab !== tab || page.depth !== depth) {
    const anim = tab === page.tab ? (depth > page.depth ? " anim-push" : " anim-pop") : "";
    setPage({ tab, depth, anim });
  }
  const anim = page.anim;

  let screen: ReactNode;
  if (location === "/done") screen = <DoneScreen key="done" anim={anim} now={now} />;
  else if (tab === "settings") {
    screen = section === undefined ? <Redirect to={settingsUrl()} replace /> : <SettingsScreen key={section ?? "home"} section={section} anim={anim} onSignOut={onSignOut} />;
  } else if (tab === "next") screen = <NextScreen key="next" anim={anim} now={now} />;
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
      {batch.on ? (
        <BatchBar />
      ) : (
        <nav className="tabbar">
          {tabButton("next", "接下来", CircleCheck, "/")}
          {tabButton("folders", "文件夹", FolderIcon, "/folders")}
          <button className={`tab${tab === "settings" ? " on" : ""}`} onClick={() => navigate(settingsUrl())}>
            <span className="tab-ic">
              <span className="avatar">{[...user.display_name][0]}</span>
            </span>
            设置
          </button>
        </nav>
      )}
      {selected && board.task(selected) && (
        <Sheet label="事项详情" onClose={close}>
          <Detail id={selected} now={now} onClose={close} />
        </Sheet>
      )}
    </>
  );
}

/** A page with a large title that gives way to a small one in a frosted bar once scrolled past.
 * With `fill`, the page doesn't scroll and its last child takes the height left, as the graph does.
 * With `choosing`, its tasks are being chosen, and the bar is for that. */
function Screen(p: { title: string; nav?: ReactNode; anim?: string; fill?: boolean; choosing?: boolean; children: ReactNode; after?: ReactNode }) {
  const { title, nav, anim = "", fill, choosing, children, after } = p;
  const [scrolled, setScrolled] = useState(false);
  return (
    <section className={`screen${scrolled ? " scrolled" : ""}${choosing ? " choosing" : ""}${anim}`}>
      <header className="nav" data-tauri-drag-region="deep">
        {choosing ? (
          <BatchHead />
        ) : (
          <>
            {nav ?? <span />}
            <span className="nav-title">{title}</span>
            <span />
          </>
        )}
      </header>
      <div className={`scroll${fill ? " fill" : ""}`} onScroll={(e) => setScrolled(e.currentTarget.scrollTop > 36)}>
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
  useBack("page", up);
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
        <Tip label="已完成">
          <button className="icon-btn" onClick={() => navigate("/done")}>
            <Icon of={History} />
          </button>
        </Tip>
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

/** The settings' home page, or a section of them. */
function SettingsScreen({ section, anim, onSignOut }: { section: SettingsSection | null; anim: string; onSignOut(): void }) {
  const title = section ? settingsTitle(section) : "设置";
  return (
    <Screen title={title} nav={section && <Back to={settingsUrl()} label="设置" />} anim={anim}>
      <div className="hero">
        <h1 className="large-title">{title}</h1>
      </div>
      {section ? <SettingsPage section={section} onSignOut={onSignOut} /> : <SettingsHome />}
    </Screen>
  );
}

function DoneScreen({ anim, now }: { anim: string; now: Date }) {
  const { board } = useView();
  const batch = useBatch();
  const any = board.snapshot.tasks.some((t) => t.done_at !== null);
  return (
    <Screen title="已完成" nav={<Back to="/" label="接下来" />} anim={anim} choosing={batch.on}>
      <div className="hero">
        <h1 className="large-title">已完成</h1>
        {any && !batch.on && (
          <Tip label="批量操作">
            <button className="icon-btn" onClick={() => batch.start()}>
              <Icon of={ListChecks} />
            </button>
          </Tip>
        )}
      </div>
      <DoneBody now={now} />
    </Screen>
  );
}

/** A folder, or the root (`null`). The list and the graph share the page, so switching between them
 * changes only what is under the switch. The root is where the folders are reached from, so it is
 * always a list, with the search over all tasks. */
function FolderScreen({ folder, anim, now }: { folder: FolderId | null; anim: string; now: Date }) {
  const { board } = useView();
  const [view] = useFolderView();
  const [query, setQuery] = useState("");
  const f = folder ? board.folder(folder)! : null;
  const title = f ? f.name : "文件夹";
  const nav = f && <Back to={folderUrl(f.parent_id)} label={f.parent_id ? board.folderName(f.parent_id) : "文件夹"} />;
  const batch = useBatch();
  const graph = f !== null && view === "graph";
  return (
    <Screen title={title} nav={nav} anim={anim} fill={graph} choosing={batch.on} after={!graph && !batch.on && <QuickAdd folder={folder} />}>
      <div className="hero">
        <h1 className="large-title">{title}</h1>
        {!batch.on && <FolderMenu folder={folder} />}
      </div>
      {batch.on ? null : f ? (
        <div className="view-seg">
          <FolderViewSeg />
        </div>
      ) : (
        <label className="search" style={{ marginTop: 12 }}>
          <Icon of={Search} size={18} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索全部事项" autoComplete="off" />
        </label>
      )}
      {graph ? (
        <FolderGraph folder={f.id} across={false} now={now} />
      ) : query.trim() && !batch.on ? (
        <SearchResults query={query} now={now} />
      ) : (
        <FolderBody folder={folder} now={now} />
      )}
    </Screen>
  );
}
