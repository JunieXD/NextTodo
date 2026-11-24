// Folder colours (docs/ui.md#文件夹的颜色和图标): `#rrggbb`, its red, green and blue, its hue,
// saturation and value, and what is drawn with it in each theme.

export type Hex = `#${string}`;
/** Red, green and blue, 0–255. */
export type Rgb = [number, number, number];
/** Hue 0–360, saturation and value 0–1. */
export type Hsv = [number, number, number];

/** `#rrggbb` of what was typed, with or without `#`, in either case. Null if it is none. */
export function parseHex(s: string): Hex | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(s.trim());
  return m ? `#${m[1].toLowerCase()}` : null;
}

export function rgbOf(hex: Hex): Rgb {
  const part = (i: number) => parseInt(hex.slice(i, i + 2), 16);
  return [part(1), part(3), part(5)];
}

export const hexOf = (rgb: Rgb): Hex => `#${rgb.map((c) => c.toString(16).padStart(2, "0")).join("")}`;

export function rgbOfHsv([h, s, v]: Hsv): Rgb {
  const part = (n: number) => {
    const k = (n + h / 60) % 6;
    return Math.round(255 * v * (1 - s * Math.max(0, Math.min(k, 4 - k, 1))));
  };
  return [part(5), part(3), part(1)];
}

export function hsvOfRgb([r, g, b]: Rgb): Hsv {
  const v = Math.max(r, g, b);
  const d = v - Math.min(r, g, b);
  const h = d === 0 ? 0 : v === r ? ((g - b) / d + 6) % 6 : v === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, v === 0 ? 0 : d / v, v / 255];
}

const linear = (c: number) => (c / 255 <= 0.04045 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4);
const encoded = (c: number, round: (n: number) => number) => round(255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055));
/** Relative luminance, as WCAG has it. */
const luminance = (hex: Hex) => {
  const [r, g, b] = rgbOf(hex).map(linear);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
export const contrast = (a: Hex, b: Hex) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

const WHITE: Hex = "#ffffff";
/** The dark icon on a light fill: the light theme's `--on-surface`. */
const DARK_ICON: Hex = "#171d1a";
/** The page in each theme, its groups' `--group-bg`: in the dark theme, the lightest of the accents'. */
export const LIGHT_PAGE: Hex = "#ffffff";
export const DARK_PAGE: Hex = "#1b2122";
/** How far from the page an icon drawn on it is, at least. On the light page it may be nearer: yellow
 * keeps more of its yellow, and the dark theme's least would take it to brown. */
export const ON_LIGHT = 2.2;
export const ON_DARK = 3;
const MOST = (luminance(LIGHT_PAGE) + 0.05) / ON_LIGHT - 0.05;
const LEAST = ON_DARK * (luminance(DARK_PAGE) + 0.05) - 0.05;

/** What folder colour `hex` draws: `fill` behind its icon, `on` the icon on it, white unless that is
 * not seen, and `light`, `dark` the icon drawn on the page in each theme: the colour, mixed with black
 * or white as far as it takes to be seen there. */
export function roles(hex: Hex) {
  const y = luminance(hex);
  const [r, g, b] = rgbOf(hex).map(linear);
  // Mixed in linear light, so that the luminance goes straight to `target` and the hue stays; rounded
  // on toward black or white, not to fall short of it.
  const toward = (target: number, to: number) => {
    const t = (target - y) / (to - y);
    const mix = (c: number) => encoded(c + (to - c) * t, to ? Math.ceil : Math.floor);
    return hexOf([mix(r), mix(g), mix(b)]);
  };
  return {
    fill: hex,
    on: contrast(hex, WHITE) < ON_LIGHT ? DARK_ICON : WHITE,
    light: y > MOST ? toward(MOST, 0) : hex,
    dark: y < LEAST ? toward(LEAST, 1) : hex,
  };
}
