// The settings: a home page that lists the sections, each a page of its own (docs/ui.md#设置).

import { Bell, ChevronRight, HardDrive, History, Info, KeyRound, LayoutGrid, ListChecks, Lock, type LucideIcon, MonitorSmartphone, Palette, Paperclip, Server, Sparkles } from "lucide-react";
import { type ReactNode } from "react";
import { useLocation } from "wouter";
import { app, phoneApp } from "../api/http";
import * as local from "../local";
import { useServerVersions } from "../server-update";
import { useSession } from "../session";
import { Icon } from "../ui/icons";
import { newer, usePending } from "../updates";
import { About } from "./About";
import { AiPage, useAiSummary } from "./AiSettings";
import { NotificationSettings, SystemSettings } from "./Background";
import { DesktopSystem } from "./DesktopSystem";
import { RemindSettings } from "./RemindSettings";
import { syncLine, useDriveSync } from "./DriveSync";
import { LockPage } from "./Lock";
import { LogPage } from "./Log";
import { ServerPage } from "./ServerSettings";
import { WidgetSettings } from "./WidgetSettings";
import { type SettingsSection, settingsUrl } from "./nav";
import { Account } from "./AccountSettings";
import { LocalMode } from "./LocalSettings";
import { Appearance } from "./AppearanceSettings";
import { Lists } from "./ListSettings";
import { Agent } from "./AgentSettings";

const TITLES: Record<SettingsSection, string> = {
  account: "账号",
  local: "本机模式",
  log: "改动记录",
  lock: "隐私锁",
  appearance: "外观",
  lists: "列表和添加",
  reminders: "提醒",
  widget: "小组件",
  system: "系统",
  ai: "AI 整理",
  agent: "agent 接入",
  server: "服务器",
  about: "关于",
};

const ICONS: Record<Exclude<SettingsSection, "account">, LucideIcon> = {
  local: HardDrive,
  log: History,
  lock: Lock,
  appearance: Palette,
  lists: ListChecks,
  reminders: Bell,
  widget: LayoutGrid,
  system: MonitorSmartphone,
  ai: Sparkles,
  agent: KeyRound,
  server: Server,
  about: Info,
};

/** In local mode the server is the app's, and has nothing of its own to set but the attachments. */
export const settingsTitle = (section: SettingsSection) => (section === "server" && local.inLocalMode() ? "附件" : TITLES[section]);

const icon = (section: Exclude<SettingsSection, "account">) => (section === "server" && local.inLocalMode() ? Paperclip : ICONS[section]);

/** The sections here, in the home page's groups. Agents reach no phone in local mode: its server
 * listens on the phone alone, which has no command line. */
function useSections(): SettingsSection[][] {
  const { user } = useSession();
  const inLocal = local.inLocalMode();
  // Phones have a lock screen of their own (docs/design.md#隐私锁).
  const groups: SettingsSection[][] = [
    [inLocal ? "local" : "account", "log", ...(phoneApp ? [] : (["lock"] as const))],
    // Only the apps run on a system to set things of: the web has the browser's.
    ["appearance", "lists", "reminders", ...(app === "android" || app === "desktop" ? (["widget", "system"] as const) : [])],
  ];
  groups.push(!inLocal || app === "desktop" ? ["ai", "agent"] : ["ai"]);
  // The server's own: its admin's, or in local mode the user's, who has it alone.
  if (user.is_admin || inLocal) groups.push(["server"]);
  groups.push(["about"]);
  return groups;
}

/** The section at `location`: null on the home page, undefined for one not here, such as the
 * invites for someone not an admin. */
export function useSectionAt(location: string): SettingsSection | null | undefined {
  const sections = useSections().flat();
  return location === settingsUrl() ? null : sections.find((s) => location === settingsUrl(s));
}

export function SettingsHome() {
  const [, navigate] = useLocation();
  const { user } = useSession();
  const ai = useAiSummary();
  const pending = usePending();
  const server = useServerVersions();
  const upgradable = user.is_admin && !local.inLocalMode() && server?.latest && newer(server.latest, server.server);
  // Only what is worth seeing without opening it.
  const values: Partial<Record<SettingsSection, string>> = {
    ai,
    server: upgradable ? "可升级" : undefined,
    about: pending ? "有新版本" : undefined,
  };
  const open = (s: SettingsSection) => () => navigate(settingsUrl(s));
  return (
    <div className="settings-page">
      {useSections().map((group) => (
        <div key={group[0]} className="group">
          {group.map((s) => {
            if (s === "account") {
              return (
                <Entry key={s} onClick={open(s)}>
                  <span className="avatar" style={{ margin: 0 }}>
                    {[...user.display_name][0]}
                  </span>
                  <span className="folder-main">
                    <span>{user.display_name}</span>
                    <span className="folder-sub">用户名 {user.username}</span>
                  </span>
                </Entry>
              );
            }
            return (
              <Entry key={s} onClick={open(s)}>
                <span className="token-ic">
                  <Icon of={icon(s)} size={18} />
                </span>
                {s === "local" ? (
                  <LocalCard />
                ) : (
                  <>
                    <span className="row-name">{settingsTitle(s)}</span>
                    <span className="row-value long">{values[s]}</span>
                  </>
                )}
              </Entry>
            );
          })}
        </div>
      ))}
    </div>
  );
}

/** A row on the home page that leads to a section. */
function Entry({ onClick, children }: { onClick(): void; children: ReactNode }) {
  return (
    <button className="row" onClick={onClick}>
      <div className="row-inner">
        {children}
        <span className="chev">
          <Icon of={ChevronRight} size={18} />
        </span>
      </div>
    </button>
  );
}

export function SettingsPage({ section, onSignOut }: { section: SettingsSection; onSignOut(): void }) {
  const pages: Record<SettingsSection, () => ReactNode> = {
    account: () => <Account onSignOut={onSignOut} />,
    local: () => <LocalMode />,
    log: () => <LogPage />,
    lock: () => <LockPage />,
    appearance: () => <Appearance />,
    lists: () => <Lists />,
    reminders: () => (
      <>
        <RemindSettings />
        {phoneApp && <NotificationSettings />}
      </>
    ),
    widget: () => <WidgetSettings />,
    system: () => (app === "android" ? <SystemSettings /> : <DesktopSystem />),
    ai: () => <AiPage />,
    agent: () => <Agent />,
    server: () => <ServerPage />,
    about: () => <About />,
  };
  return <div className="settings-page">{pages[section]()}</div>;
}

/** In place of the account's row: where the data is, and how syncing it through a drive goes. */
function LocalCard() {
  const [sync] = useDriveSync(false);
  return (
    <span className="folder-main">
      <span>本机模式</span>
      <span className="folder-sub">{sync ? syncLine(sync, new Date()) : " "}</span>
    </span>
  );
}
