// How the Android widgets look (docs/android.md#小组件的设置), and the colours they are drawn in, worked
// out as the widgets' `Colors` (apps/android/.../widget/Look.kt) does, for the settings' preview; the
// desktop widget takes its colours from the same (docs/desktop.md#桌面小组件).

import { type Hex, luminance, parseHex, rgbOf } from "./color";

export type WidgetTextSize = "small" | "standard" | "large";

/** The “一周” widget's days: today and the six after, or Monday to Sunday of this week. */
export type WidgetWeek = "today" | "monday";

/** A colour is `white`, `black` or `#rrggbb`. */
export interface WidgetLook {
  /** `system`, or a colour. */
  background: string;
  /** The background's, 0–100. */
  opacity: number;
  /** `auto`, or a colour. */
  text: string;
  /** What the header has. */
  title: boolean;
  refresh: boolean;
  add: boolean;
  size: WidgetTextSize;
  week: WidgetWeek;
}

/** The colours of the palette the widgets take, as `Palette.kt` has them. */
const PALETTES = {
  light: {
    surface: "#F5FBF6",
    onSurface: "#171D1A",
    onSurfaceVariant: "#404944",
    outline: "#707973",
    divider: "#DDE4DF",
    primary: "#1B6B51",
    onPrimary: "#FFFFFF",
    error: "#BA1A1A",
    warn: "#8A5100",
  },
  dark: {
    surface: "#0F1512",
    onSurface: "#DEE4DF",
    onSurfaceVariant: "#BFC9C2",
    outline: "#89938D",
    divider: "#2A302D",
    primary: "#8BD6B6",
    onPrimary: "#003827",
    error: "#FFB4AB",
    warn: "#FFB870",
  },
} satisfies Record<string, Record<string, Hex>>;

const colorOf = (s: string): Hex => (s === "white" ? "#ffffff" : s === "black" ? "#000000" : parseHex(s)!);

/** Whether dark text reads better on `c` than light: it is nearer white than black by contrast. */
const light = (c: Hex) => luminance(c) > 0.179;

const alpha = (c: Hex, a: number) => `rgb(${rgbOf(c).join(" ")} / ${a})`;

/** The colours the widgets are drawn in as `look` has them, the system `dark` or not: the text and the
 * marks those of the palette that go on a light background or on a dark one; one's own text colour,
 * fainter for what is less to the fore. */
export function widgetColors(look: Pick<WidgetLook, "background" | "opacity" | "text">, dark: boolean) {
  const system = PALETTES[dark ? "dark" : "light"];
  const ground = look.background === "system" ? system.surface : colorOf(look.background);
  const own = look.text === "auto" ? null : colorOf(look.text);
  const tone = own
    ? PALETTES[light(own) ? "dark" : "light"]
    : // Mostly the wallpaper shows through, light or dark no one knows: as the system is.
      look.background === "system" || look.opacity < 50
      ? system
      : PALETTES[light(ground) ? "light" : "dark"];
  const faint = (a: number, c: Hex) => (own ? alpha(own, a) : c);
  return {
    background: alpha(ground, look.opacity / 100),
    text: own ?? tone.onSurface,
    secondary: faint(0.7, tone.onSurfaceVariant),
    done: faint(0.5, tone.outline),
    divider: faint(0.15, tone.divider),
    primary: tone.primary,
    onPrimary: tone.onPrimary,
    overdue: tone.error,
    today: tone.warn,
    /** Under the text where the desktop widget can't be see-through: a row as it is swiped. */
    surface: tone.surface,
  };
}
