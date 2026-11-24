// A colour of one's own (docs/ui.md#文件夹的颜色和图标): saturation and value on the square, the hue on
// the bar, or typed as `#rrggbb` or as its red, green and blue.

import { type CSSProperties, type KeyboardEvent, type PointerEvent, useState } from "react";
import { type Hex, type Hsv, hexOf, hsvOfRgb, parseHex, type Rgb, rgbOf, rgbOfHsv } from "../domain/color";

const clamp = (n: number) => Math.min(1, Math.max(0, n));

/** An area to drag on, or to step through with the arrow keys; `at` is where its thumb is, 0–1 across
 * and down. A `line` goes only across, its up and down keys too. */
function Pad({
  className,
  style,
  label,
  text,
  now,
  max,
  at,
  line = false,
  onAt,
}: {
  className: string;
  style: CSSProperties;
  label: string;
  text: string;
  now: number;
  max: number;
  at: [number, number];
  line?: boolean;
  onAt(x: number, y: number): void;
}) {
  const point = (e: PointerEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    onAt(clamp((e.clientX - r.left) / r.width), clamp((e.clientY - r.top) / r.height));
  };
  const key = (e: KeyboardEvent) => {
    const step = e.shiftKey ? 0.1 : 0.01;
    const by = new Map<string, [number, number]>([
      ["ArrowLeft", [-step, 0]],
      ["ArrowRight", [step, 0]],
      ["ArrowUp", line ? [step, 0] : [0, -step]],
      ["ArrowDown", line ? [-step, 0] : [0, step]],
    ]).get(e.key);
    if (!by) return;
    e.preventDefault();
    onAt(clamp(at[0] + by[0]), clamp(at[1] + by[1]));
  };
  return (
    <div
      className={className}
      style={style}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuetext={text}
      aria-valuenow={now}
      aria-valuemin={0}
      aria-valuemax={max}
      onPointerDown={(e) => (e.currentTarget.setPointerCapture(e.pointerId), point(e))}
      onPointerMove={(e) => e.currentTarget.hasPointerCapture(e.pointerId) && point(e)}
      onKeyDown={key}
    >
      <span className="picker-thumb" style={{ left: `${at[0] * 100}%`, top: `${at[1] * 100}%` }} />
    </div>
  );
}

const CHANNELS = ["R", "G", "B"] as const;
type Field = "hex" | (typeof CHANNELS)[number];

/** `hex` as hue, saturation and value; those it has none of, at grey or black, as in `hsv`. */
function kept([h, s]: Hsv, hex: Hex): Hsv {
  const [th, ts, tv] = hsvOfRgb(rgbOf(hex));
  return [ts && tv ? th : h, tv ? ts : s, tv];
}

export function ColorPicker({ value, onChange }: { value: Hex; onChange(hex: Hex): void }) {
  // Hue and saturation are kept apart from the colour: grey or black has none, and they stay where they
  // were dragged to.
  const [hsv, setHsv] = useState<Hsv>(() => hsvOfRgb(rgbOf(value)));
  // Changed elsewhere, on another device say, the square and the bar go to it.
  const [was, setWas] = useState(value);
  if (value !== was) {
    setWas(value);
    if (value !== hexOf(rgbOfHsv(hsv))) setHsv(kept(hsv, value));
  }
  // What is being typed in a field, until it is left: it may not be a colour yet.
  const [typed, setTyped] = useState<{ field: Field; text: string } | null>(null);
  const [h, s, v] = hsv;
  const rgb = rgbOf(value);
  const drag = (next: Hsv) => {
    setHsv(next);
    setTyped(null);
    onChange(hexOf(rgbOfHsv(next)));
  };
  const type = (field: Field, text: string, hex: Hex | null) => {
    setTyped({ field, text });
    if (!hex) return;
    setHsv(kept(hsv, hex));
    onChange(hex);
  };
  const shown = (field: Field, text: string) => (typed?.field === field ? typed.text : text);
  // A whole number, more than 255 taken as 255; anything else stays typed until it is one.
  const channel = (i: number, text: string) => {
    if (!/^\d+$/.test(text.trim())) return type(CHANNELS[i], text, null);
    const next: Rgb = [...rgb];
    next[i] = Math.min(255, Number(text.trim()));
    type(CHANNELS[i], String(next[i]), hexOf(next));
  };
  return (
    <div className="picker">
      <Pad
        className="picker-sv"
        style={{ "--hue": h, "--thumb": value }}
        label="饱和度和亮度"
        text={`饱和度 ${Math.round(s * 100)}%，亮度 ${Math.round(v * 100)}%`}
        now={Math.round(s * 100)}
        max={100}
        at={[s, 1 - v]}
        onAt={(x, y) => drag([h, x, 1 - y])}
      />
      <Pad
        className="picker-hue"
        style={{ "--thumb": `hsl(${h} 100% 50%)` }}
        label="色相"
        text={`色相 ${Math.round(h)}°`}
        now={Math.round(h)}
        max={360}
        at={[h / 360, 0.5]}
        line
        onAt={(x) => drag([x * 360, s, v])}
      />
      <div className="picker-fields">
        <label>
          #
          <input
            className="picker-hex"
            aria-label="十六进制"
            autoComplete="off"
            spellCheck={false}
            value={shown("hex", value.slice(1))}
            onChange={(e) => type("hex", e.target.value, parseHex(e.target.value))}
            onBlur={() => setTyped(null)}
          />
        </label>
        {CHANNELS.map((name, i) => (
          <label key={name}>
            {name}
            <input inputMode="numeric" value={shown(name, String(rgb[i]))} onChange={(e) => channel(i, e.target.value)} onBlur={() => setTyped(null)} />
          </label>
        ))}
      </div>
    </div>
  );
}
