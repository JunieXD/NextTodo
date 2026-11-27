// The widgets' settings: a preview on top, then how they look. On Android those of the home-screen
// widgets (docs/android/widgets.md#小组件的设置); in the desktop app the desktop widget's, shown or not, with the
// same colours (docs/desktop.md#桌面小组件).

import { Plus, RefreshCw, X } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useContext, useEffect, useState } from "react";
import { app } from "../api/http";
import { desktopWidget, widgetPageKey } from "../desktop";
import { type Hex, parseHex } from "../domain/color";
import { dueKind, formatDue } from "../domain/dates";
import { type WidgetLook, type WidgetTextSize, type WidgetWeek, widgetColors } from "../domain/widget-look";
import { NextTodo } from "../mobile";
import { nextFolders, readyIn } from "../next-folders";
import { useKept } from "../kept";
import { useView } from "../session";
import { useDark } from "../theme";
import { ColorPicker } from "../ui/color-picker";
import { Seg, Switch } from "../ui/controls";
import { forUser, RootContext, Tip, useFeedback } from "../ui/feedback";
import { useMedia } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { type DesktopLook, setDesktopLook, useDesktopLook } from "../widget";
import { setZoom, useZoom } from "../zoom";
import { useTimeGridOn } from "./calendar";
import { PAGES, PanelPageName } from "./Panel";

const SCALES: Record<WidgetTextSize, number> = { small: 0.88, standard: 1, large: 1.15 };

const SIZES: [WidgetTextSize, string][] = [
  ["small", "小"],
  ["standard", "标准"],
  ["large", "大"],
];

type Choice = "system" | "auto" | "white" | "black" | "own";

/** Which of `first`, white, black or one's own colour `color` is. */
const choice = (color: string): Choice => (color === "system" || color === "auto" || color === "white" || color === "black" ? color : "own");

/** A colour of one's own to start from, the widgets' green. */
const OWN: Hex = "#1b6b51";

export function WidgetSettings() {
  return app === "desktop" ? <DesktopWidget /> : <AndroidWidgets />;
}

function DesktopWidget() {
  const { snack } = useFeedback();
  const look = useDesktopLook();
  const size = useZoom("widget");
  const [widgets, setWidgets] = useState<Awaited<ReturnType<typeof desktopWidget.list>> | null>(null);
  useEffect(() => {
    let active = true;
    const refresh = async () => {
      try {
        const widgets = await desktopWidget.list();
        if (active) setWidgets(widgets);
      } catch (e) {
        if (active) snack(forUser("读取桌面小组件的设置", e));
      }
    };
    const listening = desktopWidget.onChange(() => void refresh()).then(
      (unlisten) => {
        if (active) void refresh();
        return unlisten;
      },
      (e: unknown) => {
        if (active) snack(forUser("监听桌面小组件的变动", e));
      },
    );
    return () => {
      active = false;
      void listening.then((unlisten) => unlisten?.());
    };
  }, [snack]);
  const root = useContext(RootContext);
  const [grid] = useTimeGridOn();
  const add = (page: string) => void desktopWidget.add(page).catch((e: unknown) => snack(forUser("添加桌面小组件", e)));
  return (
    <>
      <h2 className="group-title">桌面上的小组件</h2>
      <div className="group settings widget-list">
        {widgets?.labels.map((label) => (
          <DesktopWidgetRow key={label} label={label} />
        ))}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger className="row" disabled={!widgets || widgets.full}>
            <span className="row-inner">
              <Icon of={Plus} size={18} />
              <span className="row-name">添加小组件</span>
            </span>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal container={root}>
            <DropdownMenu.Content className="menu" align="start" sideOffset={4}>
              {PAGES.filter((p) => grid || p.path !== "/calendar/time").map((p) => (
                <DropdownMenu.Item key={p.path} className="menu-item" onSelect={() => add(p.path)}>
                  <Icon of={p.icon} />
                  {p.name}
                </DropdownMenu.Item>
              ))}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      </div>
      <Preview look={look} dark={useDark()} size={size} head={{ title: true, refresh: false, add: false }} />
      <Colors look={look} change={(to) => setDesktopLook({ ...look, ...to })} />
      <TextSize value={size} onChange={(z) => void setZoom("widget", z).catch((e: unknown) => snack(forUser("设置文字大小", e)))} />
    </>
  );
}

function DesktopWidgetRow({ label }: { label: string }) {
  const [path] = useKept(widgetPageKey(label));
  const { snack } = useFeedback();
  const remove = () => void desktopWidget.remove(label).catch((e: unknown) => snack(forUser("移除桌面小组件", e)));
  return (
    <div className="row">
      <div className="row-inner">
        <span className="row-name truncate">
          <PanelPageName path={path ?? "/"} />
        </span>
        <Tip label="移除小组件">
          <button type="button" className="icon-btn sm" onClick={remove}>
            <Icon of={X} size={18} />
          </button>
        </Tip>
      </div>
    </div>
  );
}

/** The look last chosen and its saving, a moment later: kept apart from the page, which may be left and
 * opened again before then. */
let chosen: WidgetLook | null = null;
let saving: number | undefined;

function AndroidWidgets() {
  const { snack } = useFeedback();
  const [look, setLook] = useState(chosen);
  const dark = useMedia("(prefers-color-scheme: dark)");
  useEffect(() => {
    if (!chosen) NextTodo.widgetLook().then(setLook, (e: unknown) => snack(forUser("读取小组件的外观", e)));
  }, [snack]);
  if (!look) return null;
  // The preview at once, the widgets a moment after the last change: dragging redraws them once.
  const change = (to: Partial<WidgetLook>) => {
    const next = { ...look, ...to };
    chosen = next;
    setLook(next);
    clearTimeout(saving);
    saving = window.setTimeout(
      () =>
        void NextTodo.setWidgetLook(next).catch((e: unknown) => {
          // Opened again, the page shows what the widgets have.
          if (chosen === next) chosen = null;
          snack(forUser("设置小组件的外观", e));
        }),
      300,
    );
  };
  return (
    <>
      <Preview look={look} dark={dark} size={look.size} head={look} />
      <Colors look={look} change={change} />
      <h2 className="group-title">标题栏</h2>
      <div className="group settings">
        <Switch label="标题" sub="点按打开 App" on={look.title} onChange={(title) => change({ title })} />
        <Switch label="刷新按钮" sub="关闭后仍会定时自动刷新" on={look.refresh} onChange={(refresh) => change({ refresh })} />
        <Switch label="添加按钮" sub="“日程”没有添加按钮" on={look.add} onChange={(add) => change({ add })} />
      </div>
      <TextSize value={look.size} onChange={(size) => change({ size })} />
      <h2 className="group-title">“一周”的日子</h2>
      <div style={{ margin: "0 16px 16px" }}>
        <Seg<WidgetWeek>
          label="“一周”的日子"
          items={[
            ["today", "从今天起 7 天"],
            ["monday", "本周一到周日"],
          ]}
          value={look.week}
          onChange={(week) => change({ week })}
        />
      </div>
    </>
  );
}

/** The background's colour and opacity, and the text's colour. */
function Colors({ look, change }: { look: DesktopLook; change(to: Partial<DesktopLook>): void }) {
  const colors = (key: "background" | "text", first: [Choice, string]) => {
    const value = look[key];
    const own = parseHex(value);
    return (
      <>
        <div style={{ margin: "0 16px" }}>
          <Seg<Choice>
            label={key === "background" ? "背景颜色" : "文字颜色"}
            items={[first, ["white", "白色"], ["black", "黑色"], ["own", "自定义"]]}
            value={choice(value)}
            onChange={(c) => change({ [key]: c === "own" ? OWN : c })}
          />
        </div>
        {own && (
          <div className="widget-picker">
            <ColorPicker value={own} onChange={(c) => change({ [key]: c })} />
          </div>
        )}
      </>
    );
  };
  return (
    <>
      <h2 className="group-title">背景颜色</h2>
      {colors("background", ["system", app === "desktop" ? "跟随主题" : "跟随系统"])}
      <div className="group settings">
        <label className="row">
          <div className="row-inner">
            <span className="row-name">不透明度</span>
            <input
              className="range"
              type="range"
              min={0}
              max={100}
              step={5}
              value={look.opacity}
              aria-valuetext={`${look.opacity}%`}
              onChange={(e) => change({ opacity: Number(e.target.value) })}
            />
            <span className="row-value range-value">{look.opacity}%</span>
          </div>
        </label>
      </div>
      <h2 className="group-title">文字颜色</h2>
      {colors("text", ["auto", "自动"])}
    </>
  );
}

function TextSize({ value, onChange }: { value: WidgetTextSize; onChange(size: WidgetTextSize): void }) {
  return (
    <>
      <h2 className="group-title">文字大小</h2>
      <div style={{ margin: "0 16px 16px" }}>
        <Seg<WidgetTextSize> label="文字大小" items={SIZES} value={value} onChange={onChange} />
      </div>
    </>
  );
}

/** The “接下来” widget as `look` has it, its first three tasks, over a wallpaper half light and half
 * dark: how the opacity and the text colour come out on either. As the system or the app is light or
 * dark, as the widgets are. */
function Preview({
  look,
  dark,
  size,
  head,
}: {
  look: DesktopLook;
  dark: boolean;
  size: WidgetTextSize;
  head: Pick<WidgetLook, "title" | "refresh" | "add">;
}) {
  const { board } = useView();
  const tasks = readyIn(board, nextFolders(board)).slice(0, 3);
  const c = widgetColors(look, dark);
  const now = new Date();
  const headed = head.title || head.refresh || head.add;
  return (
    <div className="widget-stage" aria-label="小组件预览" role="img">
      <div className="widget-wall">
        <div
          className="widget-card"
          style={{
            paddingTop: headed ? 0 : 10,
            background: c.background,
            color: c.text,
            "--scale": SCALES[size],
            "--w-divider": c.divider,
            "--w-primary": c.primary,
          }}
        >
          {headed && (
            <div className={`widget-head${head.title ? "" : " low"}`}>
              <b>{head.title && "接下来"}</b>
              <span className="widget-buttons" style={{ color: c.secondary }}>
                {head.refresh && <Icon of={RefreshCw} size={18} />}
                {head.add && <Icon of={Plus} size={20} />}
              </span>
            </div>
          )}
          {tasks.length ? (
            tasks.map((t) => {
              // As the widget shows it (docs/design/dates.md#后续的截止).
              const by = board.dueBy().get(t.id);
              const due = t.due ?? by?.due;
              const kind = due && dueKind(due, now);
              return (
                <div key={t.id} className="widget-row">
                  <span className="widget-circle" />
                  <span className="widget-title">{t.title}</span>
                  {due && (
                    <span className="widget-meta" style={{ color: kind === "overdue" ? c.overdue : kind === "today" ? c.today : c.secondary }}>
                      {t.due ? "" : "后续 "}
                      {formatDue(due, now)}
                    </span>
                  )}
                  {t.due && by && (
                    <span className="widget-meta" style={{ color: c.today, fontWeight: 500 }}>
                      晚于后续
                    </span>
                  )}
                </div>
              );
            })
          ) : (
            <div className="widget-note" style={{ color: c.secondary }}>
              暂无可做的事项
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
