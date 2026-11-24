// The Android widgets' settings (docs/android.md#小组件的设置): a preview on top, then how they look.

import { Plus, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { type Hex, parseHex } from "../domain/color";
import { dueKind, formatDue } from "../domain/dates";
import { type WidgetLook, type WidgetTextSize, widgetColors } from "../domain/widget-look";
import { NextTodo } from "../mobile";
import { nextFolders, readyIn } from "../next-folders";
import { useView } from "../session";
import { ColorPicker } from "../ui/color-picker";
import { Seg, Switch } from "../ui/controls";
import { forUser, useFeedback } from "../ui/feedback";
import { useMedia } from "../ui/hooks";
import { Icon } from "../ui/icons";

const SCALES: Record<WidgetTextSize, number> = { small: 0.88, standard: 1, large: 1.15 };

type Choice = "system" | "auto" | "white" | "black" | "own";

/** Which of `first`, white, black or one's own colour `color` is. */
const choice = (color: string): Choice => (color === "system" || color === "auto" || color === "white" || color === "black" ? color : "own");

/** A colour of one's own to start from, the widgets' green. */
const OWN: Hex = "#1b6b51";

/** The look last chosen and its saving, a moment later: kept apart from the page, which may be left and
 * opened again before then. */
let chosen: WidgetLook | null = null;
let saving: number | undefined;

export function WidgetSettings() {
  const { snack } = useFeedback();
  const [look, setLook] = useState(chosen);
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
      <Preview look={look} />
      <h2 className="group-title">背景颜色</h2>
      {colors("background", ["system", "跟随系统"])}
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
      <h2 className="group-title">标题栏</h2>
      <div className="group settings">
        <Switch label="标题" sub="点了打开 App" on={look.title} onChange={(title) => change({ title })} />
        <Switch label="刷新按钮" sub="关了也会随 App 和每 15 分钟一次自动刷新" on={look.refresh} onChange={(refresh) => change({ refresh })} />
        <Switch label="添加按钮" sub="“日程”没有添加按钮" on={look.add} onChange={(add) => change({ add })} />
      </div>
      <h2 className="group-title">文字大小</h2>
      <div style={{ margin: "0 16px 16px" }}>
        <Seg<WidgetTextSize>
          label="文字大小"
          items={[
            ["small", "小"],
            ["standard", "标准"],
            ["large", "大"],
          ]}
          value={look.size}
          onChange={(size) => change({ size })}
        />
      </div>
    </>
  );
}

/** The “接下来” widget as `look` has it, its first three tasks, over a wallpaper half light and half
 * dark: how the opacity and the text colour come out on either. As the system is light or dark, as the
 * widgets are. */
function Preview({ look }: { look: WidgetLook }) {
  const { board } = useView();
  const tasks = readyIn(board, nextFolders(board)).slice(0, 3);
  const c = widgetColors(look, useMedia("(prefers-color-scheme: dark)"));
  const now = new Date();
  const headed = look.title || look.refresh || look.add;
  return (
    <div className="widget-stage" aria-label="小组件预览" role="img">
      <div
        className="widget-card"
        style={{
          paddingTop: headed ? 0 : 10,
          background: c.background,
          color: c.text,
          "--scale": SCALES[look.size],
          "--w-divider": c.divider,
          "--w-primary": c.primary,
        }}
      >
        {headed && (
          <div className={`widget-head${look.title ? "" : " low"}`}>
            <b>{look.title && "接下来"}</b>
            <span className="widget-buttons" style={{ color: c.secondary }}>
              {look.refresh && <Icon of={RefreshCw} size={18} />}
              {look.add && <Icon of={Plus} size={20} />}
            </span>
          </div>
        )}
        {tasks.length ? (
          tasks.map((t) => {
            const kind = t.due && dueKind(t.due, now);
            return (
              <div key={t.id} className="widget-row">
                <span className="widget-circle" />
                <span className="widget-title">{t.title}</span>
                {t.due && (
                  <span className="widget-meta" style={{ color: kind === "overdue" ? c.overdue : kind === "today" ? c.today : c.secondary }}>
                    {formatDue(t.due, now)}
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
  );
}
