// The look (docs/ui/settings.md): the theme, the accent colour, the calendar and, in the desktop app, the zoom.

import { useState } from "react";
import { app, desktopOs } from "../api/http";
import { type Preset, type Theme, hueOf, isPreset, primaryOf, setAccent, setTheme, startOwn, tuneAccent, useAccent, useDark, useTheme } from "../theme";
import { type Zoom, type Zoomed, setZoom, useZoom } from "../zoom";
import { ColorPicker } from "../ui/color-picker";
import { Seg, Swatch, Switch } from "../ui/controls";
import { useTimeGridOn } from "./calendar";
import { useHolidayMarks } from "./daynote";
import { forUser, useFeedback } from "../ui/feedback";

const THEMES: [Theme, string][] = [
  ["system", "跟随系统"],
  ["light", "浅色"],
  ["dark", "深色"],
];

const ACCENTS: ["emerald" | Preset, string][] = [
  ["emerald", "翡翠绿"],
  ["lime", "草绿"],
  ["cyan", "青"],
  ["blue", "蓝"],
  ["indigo", "靛蓝"],
  ["purple", "紫"],
  ["pink", "粉"],
];

export function Appearance() {
  const theme = useTheme();
  const [grid, setGrid] = useTimeGridOn();
  const [marks, setMarks] = useHolidayMarks();
  const accent = useAccent();
  const dark = useDark();
  const own = accent === "emerald" || isPreset(accent) ? null : accent;
  // HCT hues from the red of what is overdue to the amber of what is due today.
  const hue = own && hueOf(own);
  const [picking, setPicking] = useState(false);
  return (
    <>
      <h2 className="group-title">主题</h2>
      <div style={{ margin: "0 16px" }}>
        <Seg<Theme> label="主题" items={THEMES} value={theme} onChange={setTheme} />
      </div>
      <h2 className="group-title">主题色</h2>
      <div className="color-choices accents">
        {ACCENTS.map(([a, name]) => (
          <Swatch
            key={a}
            label={name}
            color={a === "emerald" ? "var(--accent-emerald)" : primaryOf(a, dark)}
            chosen={a === accent}
            onChoose={() => (setAccent(a), setPicking(false))}
          />
        ))}
        <Swatch
          label="自定义"
          color={own ? primaryOf(own, dark) : undefined}
          rainbow
          chosen={own !== null}
          onChoose={() => (own ? setPicking(!picking) : (setAccent(startOwn()), setPicking(true)))}
        />
      </div>
      {own && (
        <div hidden={!picking}>
          <ColorPicker value={own} onChange={tuneAccent} hueOnly />
          {hue !== null && hue >= 10 && hue <= 100 && <p className="group-note">与逾期的红色、今天到期的琥珀色相近，可能难以区分</p>}
        </div>
      )}
      {app === "desktop" && <Zooms />}
      <h2 className="group-title">日历</h2>
      <div className="group settings">
        <Switch label="时间轴视图" sub="在日历中按小时查看一周的事项" on={grid} onChange={setGrid} />
        <Switch label="放假和补班" sub="在日历中标出国家规定的放假和补班" on={marks} onChange={setMarks} />
      </div>
    </>
  );
}

const ZOOMS: [Zoom, string][] = [
  ["small", "小"],
  ["standard", "标准"],
  ["large", "大"],
];

/** How large each window of the desktop app draws (docs/desktop.md#界面大小). */
function Zooms() {
  const { snack } = useFeedback();
  const zooms: [Zoomed, string, Zoom][] = [
    ["main", "主窗口", useZoom("main")],
    ["panel", desktopOs === "macOS" ? "菜单栏小窗" : "托盘小窗", useZoom("panel")],
  ];
  return (
    <>
      <h2 className="group-title">界面大小</h2>
      <div className="group settings">
        {zooms.map(([w, name, value]) => (
          <div key={w} className="row">
            <div className="row-inner controls">
              {name}
              <span className="row-value">
                <Seg<Zoom> small label={`${name}的界面大小`} items={ZOOMS} value={value} onChange={(z) => void setZoom(w, z).catch((e: unknown) => snack(forUser("设置界面大小", e)))} />
              </span>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
