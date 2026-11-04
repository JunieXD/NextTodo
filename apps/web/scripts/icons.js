// Draws the logo (docs/ui.md#产品图标) and writes every icon made from it: the web's, Android's and
// the desktop app's. Run it from apps/web with `node scripts/icons.js` after changing the logo; it
// needs rsvg-convert (librsvg) and macOS's iconutil.
//
// The logo is a list as the app draws one: a rail with three tasks on it, done, ready and waiting,
// each with its title bar. Every shape is a path, so the same drawing serves SVG and Android's
// vector drawables, which have no circles, dashes or masks.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const ANDROID = "../android/app/src/main/res/drawable";
const DESKTOP = "../desktop/src-tauri/icons";

// ---------- the drawing, on a 1024 tile ----------

const GREEN = ["#2E8E6C", "#14563F"]; // the tile's gradient, top left to bottom right
const MINT = "#A6F2D1";
const X = 332; // the rail
const Y = [294, 514, 734]; // the tasks

const f = (n) => +n.toFixed(2);
const pt = ([x, y]) => `${f(x)},${f(y)}`;
const circle = (cx, cy, r) => `M${pt([cx - r, cy])}A${r},${r} 0 1 0 ${pt([cx + r, cy])}A${r},${r} 0 1 0 ${pt([cx - r, cy])}Z`;
const line = (a, b) => `M${pt(a)}L${pt(b)}`;
const arc = (cx, cy, r, from, to) => {
  const at = (deg) => [cx + r * Math.cos((deg * Math.PI) / 180), cy + r * Math.sin((deg * Math.PI) / 180)];
  return `M${pt(at(from))}A${r},${r} 0 0 1 ${pt(at(to))}`;
};

/** The outline of a check drawn with a round pen of width `w`, to cut out of the done task. */
function checkOutline(cx, cy, r, w) {
  const p = [[cx - 0.42 * r, cy + 0.02 * r], [cx - 0.1 * r, cy + 0.33 * r], [cx + 0.45 * r, cy - 0.3 * r]];
  const h = w / 2;
  const unit = (a, b) => {
    const d = Math.hypot(b[0] - a[0], b[1] - a[1]);
    return [(b[0] - a[0]) / d, (b[1] - a[1]) / d];
  };
  const add = (a, v, k) => [a[0] + v[0] * k, a[1] + v[1] * k];
  const u1 = unit(p[0], p[1]), u2 = unit(p[1], p[2]);
  const n1 = [-u1[1], u1[0]], n2 = [-u2[1], u2[0]];
  // The pen turns towards the inner side; the outer side goes round the bend.
  const inner = Math.sign(u1[0] * u2[1] - u1[1] * u2[0]);
  const outer = -inner;
  const a1 = add(p[1], n1, inner * h), a2 = add(p[1], n2, inner * h);
  // Where the two inner edges meet.
  const t = ((a2[0] - a1[0]) * u2[1] - (a2[1] - a1[1]) * u2[0]) / (u1[0] * u2[1] - u1[1] * u2[0]);
  const bend = add(a1, u1, t);
  // A round end or join as two quarter turns, so no arc is ambiguous.
  const round = (c, from, mid, to) => {
    const sweep = (a, b) => (((a[0] - c[0]) * (b[1] - c[1]) - (a[1] - c[1]) * (b[0] - c[0]) > 0) ? 1 : 0);
    return `A${h},${h} 0 0 ${sweep(from, mid)} ${pt(mid)}A${h},${h} 0 0 ${sweep(mid, to)} ${pt(to)}`;
  };
  const joinMid = add(p[1], unit([0, 0], [n1[0] + n2[0], n1[1] + n2[1]]), outer * h);
  return [
    `M${pt(add(p[0], n1, outer * h))}L${pt(add(p[1], n1, outer * h))}`,
    round(p[1], add(p[1], n1, outer * h), joinMid, add(p[1], n2, outer * h)),
    `L${pt(add(p[2], n2, outer * h))}`,
    round(p[2], add(p[2], n2, outer * h), add(p[2], u2, h), add(p[2], n2, inner * h)),
    `L${pt(bend)}L${pt(add(p[0], n1, inner * h))}`,
    round(p[0], add(p[0], n1, inner * h), add(p[0], u1, -h), add(p[0], n1, outer * h)),
    "Z",
  ].join("");
}

/** The shapes, back to front. `tone` is "main" or "soft"; `alpha` dims one more. */
const SHAPES = [
  // The rail, from inside the done task to the rings' middle lines, where the rings hide its ends.
  { d: line([X, Y[0] + 60], [X, Y[1] - 66]), stroke: 40, cap: "butt", tone: "soft" },
  { d: line([X, Y[1] + 66], [X, Y[2] - 66]), stroke: 40, cap: "butt", tone: "soft" },
  // Done: a filled dot with a check cut out of it.
  { d: circle(X, Y[0], 84) + checkOutline(X, Y[0], 84, 30), fill: true, tone: "main" },
  // Ready: a ring.
  { d: circle(X, Y[1], 66), stroke: 34, cap: "butt", tone: "main" },
  // Waiting: a dashed ring, with a dash at the top over the rail's end.
  ...[0, 1, 2, 3, 4, 5].map((i) => ({ d: arc(X, Y[2], 66, -109.1 + i * 60, -70.9 + i * 60), stroke: 28, cap: "butt", tone: "soft" })),
  // The titles.
  { d: line([X + 166, Y[0]], [X + 372, Y[0]]), stroke: 64, cap: "round", tone: "main", alpha: 0.45 },
  { d: line([X + 166, Y[1]], [X + 412, Y[1]]), stroke: 64, cap: "round", tone: "main" },
  { d: line([X + 166, Y[2]], [X + 322, Y[2]]), stroke: 64, cap: "round", tone: "soft" },
];

/** Colours for each use: the tile's white and mint, or one colour with the soft parts dimmed. */
const COLOUR = { main: "#FFFFFF", soft: MINT };
const MONO = (c) => ({ main: c, soft: c, softAlpha: 0.6 });

// ---------- SVG ----------

/** A shape's colour and opacity in `colour`. */
const paint = (s, colour) => [colour[s.tone], (s.alpha ?? 1) * (s.tone === "soft" ? (colour.softAlpha ?? 1) : 1)];

function svgShapes(colour) {
  return SHAPES.map((s) => {
    const [c, alpha] = paint(s, colour);
    const attrs = s.fill
      ? [`fill="${c}"`, `fill-rule="evenodd"`]
      : [`fill="none"`, `stroke="${c}"`, `stroke-width="${s.stroke}"`, `stroke-linecap="${s.cap}"`];
    if (alpha < 1) attrs.push(`opacity="${alpha}"`);
    return `<path d="${s.d}" ${attrs.join(" ")}/>`;
  }).join("");
}

const svg = (size, body) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}">${body}</svg>\n`;
const place = ({ k, dx, dy }, body) => `<g transform="translate(${f(dx)} ${f(dy)}) scale(${+k.toPrecision(5)})">${body}</g>`;

const GRADIENT = `<linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${GREEN[0]}"/><stop offset="1" stop-color="${GREEN[1]}"/></linearGradient>`;
const SHADOW = `<filter id="s" x="-10%" y="-10%" width="120%" height="125%"><feDropShadow dx="0" dy="12" stdDeviation="14" flood-opacity=".28"/></filter>`;

/** The tile with the logo. `inset` and `radius` shape the tile inside the 1024 square; `round` false
 * fills the square for icons that are masked by the system. */
function tileSvg({ inset = 0, radius = 230, round = true, shadow = false } = {}) {
  const size = 1024 - 2 * inset;
  const tile = round
    ? `<rect x="${inset}" y="${inset}" width="${size}" height="${size}" rx="${radius}" fill="url(#g)"${shadow ? ` filter="url(#s)"` : ""}/>`
    : `<rect width="1024" height="1024" fill="url(#g)"/>`;
  return svg(1024, `<defs>${GRADIENT}${shadow ? SHADOW : ""}</defs>${tile}${place({ k: size / 1024, dx: inset, dy: inset }, svgShapes(COLOUR))}`);
}

/** The logo alone in one colour, fitted to a square of `size` with `pad` around it. */
const glyphSvg = (size, pad, colour) => svg(size, place(fit(size, pad), svgShapes(colour)));

// The logo's extent on the tile: the dots on the left, the longest title on the right.
const BOX = { left: X - 84, top: Y[0] - 84, right: X + 412 + 32, bottom: Y[2] + 66 + 14 };

/** Scale and offset that centre the logo in a square of `size` with `pad` around it. */
function fit(size, pad) {
  const w = BOX.right - BOX.left, h = BOX.bottom - BOX.top;
  const k = (size - 2 * pad) / Math.max(w, h);
  return { k, dx: (size - w * k) / 2 - BOX.left * k, dy: (size - h * k) / 2 - BOX.top * k };
}

// ---------- Android vector drawables ----------

function vector(dp, { k, dx, dy }, colour, comment) {
  const paths = SHAPES.map((s) => {
    const [c, alpha] = paint(s, colour);
    const attrs = [`android:pathData="${s.d}"`];
    if (s.fill) attrs.push(`android:fillColor="${c}"`, `android:fillType="evenOdd"`);
    else attrs.push(`android:strokeColor="${c}"`, `android:strokeWidth="${s.stroke}"`, `android:strokeLineCap="${s.cap}"`);
    if (alpha < 1) attrs.push(`android:${s.fill ? "fill" : "stroke"}Alpha="${alpha}"`);
    return `        <path\n            ${attrs.join("\n            ")} />`;
  }).join("\n");
  return `<?xml version="1.0" encoding="utf-8"?>
<!-- ${comment} Written by apps/web/scripts/icons.js; change the drawing there. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="${dp}dp"
    android:height="${dp}dp"
    android:viewportWidth="${dp}"
    android:viewportHeight="${dp}">
    <group android:translateX="${f(dx)}" android:translateY="${f(dy)}" android:scaleX="${+k.toPrecision(5)}" android:scaleY="${+k.toPrecision(5)}">
${paths}
    </group>
</vector>
`;
}

// An adaptive icon is 108dp, of which launchers show the middle 72dp: the tile's 1024 maps onto that.
const ADAPTIVE = { k: 72 / 1024, dx: 18, dy: 18 };

const background = `<?xml version="1.0" encoding="utf-8"?>
<!-- The app icon's tile. Written by apps/web/scripts/icons.js; change the drawing there. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:aapt="http://schemas.android.com/aapt"
    android:width="108dp"
    android:height="108dp"
    android:viewportWidth="108"
    android:viewportHeight="108">
    <path android:pathData="M0,0h108v108h-108z">
        <aapt:attr name="android:fillColor">
            <gradient
                android:type="linear"
                android:startX="18"
                android:startY="18"
                android:endX="90"
                android:endY="90">
                <item android:offset="0" android:color="${GREEN[0]}" />
                <item android:offset="1" android:color="${GREEN[1]}" />
            </gradient>
        </aapt:attr>
    </path>
</vector>
`;

// ---------- writing ----------

const tmp = mkdtempSync(join(tmpdir(), "nexttodo-icons-"));
const png = (svg, size, out) => {
  const src = join(tmp, "in.svg");
  writeFileSync(src, svg);
  execFileSync("rsvg-convert", ["-w", String(size), "-h", String(size), "-o", out, src]);
};

// Web: the rounded tile for browsers and the manifest; a full square where the system rounds it.
const tile = tileSvg();
const square = tileSvg({ round: false });
writeFileSync("public/icon.svg", tile);
png(tile, 192, "public/icon-192.png");
png(tile, 512, "public/icon-512.png");
png(square, 512, "public/icon-maskable-512.png");
png(square, 180, "public/apple-touch-icon.png");

// Android: the adaptive icon's layers, its monochrome form for themed icons, and the status bar icon.
writeFileSync(`${ANDROID}/ic_launcher_background.xml`, background);
writeFileSync(`${ANDROID}/ic_launcher_foreground.xml`, vector(108, ADAPTIVE, COLOUR, "The app icon's logo, in the middle 72dp a launcher shows."));
writeFileSync(`${ANDROID}/ic_launcher_monochrome.xml`, vector(108, ADAPTIVE, MONO("#FFFFFF"), "The logo in one colour, for themed icons."));
writeFileSync(`${ANDROID}/ic_stat_nexttodo.xml`, vector(24, fit(24, 2), MONO("#FFFFFF"), "The notification icon: the logo, white on transparent as Android wants."));

// macOS: the tile inset with a shadow as macOS draws app icons, and a template image for the menu bar.
const mac = tileSvg({ inset: 100, radius: 185, shadow: true });
png(mac, 32, `${DESKTOP}/32x32.png`);
png(mac, 128, `${DESKTOP}/128x128.png`);
png(mac, 256, `${DESKTOP}/128x128@2x.png`);
const set = join(tmp, "icon.iconset");
execFileSync("mkdir", [set]);
for (const s of [16, 32, 128, 256, 512]) {
  png(mac, s, join(set, `icon_${s}x${s}.png`));
  png(mac, s * 2, join(set, `icon_${s}x${s}@2x.png`));
}
execFileSync("iconutil", ["-c", "icns", "-o", `${DESKTOP}/icon.icns`, set]);
png(glyphSvg(36, 2, MONO("#000000")), 36, `${DESKTOP}/tray.png`);

// Windows: the tile in the sizes Windows asks for, in one .ico of PNGs; the tray shows it too.
const sizes = [16, 24, 32, 48, 64, 256];
const images = sizes.map((s) => {
  const out = join(tmp, `${s}.png`);
  png(tile, s, out);
  return readFileSync(out);
});
const ico = Buffer.alloc(6 + 16 * sizes.length);
ico.writeUInt16LE(1, 2); // an icon
ico.writeUInt16LE(sizes.length, 4);
let offset = ico.length;
sizes.forEach((s, i) => {
  const entry = 6 + 16 * i;
  ico.writeUInt8(s % 256, entry); // width and height, 0 for 256
  ico.writeUInt8(s % 256, entry + 1);
  ico.writeUInt16LE(1, entry + 4); // colour planes
  ico.writeUInt16LE(32, entry + 6); // bits per pixel
  ico.writeUInt32LE(images[i].length, entry + 8);
  ico.writeUInt32LE(offset, entry + 12);
  offset += images[i].length;
});
writeFileSync(`${DESKTOP}/icon.ico`, Buffer.concat([ico, ...images]));

rmSync(tmp, { recursive: true });
