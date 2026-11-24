import { Check as CheckIcon, ChevronsUpDown, Copy, type LucideIcon, Minus, Plus } from "lucide-react";
import { Select as Choice, DropdownMenu, RadioGroup, Switch as Toggle } from "radix-ui";
import { type ReactNode, useContext, useState } from "react";
import type { Status } from "../domain/board";
import { useBack } from "./back";
import { useCopy } from "./clipboard";
import { RootContext, Tip } from "./feedback";
import { Icon } from "./icons";

/** The round check box of a task: solid ring when ready, dashed while waiting, filled when done. */
export function Check({ status, large, pop, left, onToggle }: { status: Status; large?: boolean; pop?: boolean; left?: number; onToggle(): void }) {
  return (
    <button
      className={`check ${status}${large ? " lg" : ""}${pop ? " pop" : ""}`}
      style={left === undefined ? undefined : { left }}
      aria-label={status === "done" ? "标为待办" : "标为完成"}
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
    >
      <Icon of={CheckIcon} size={large ? 18 : 15} width={2.6} />
    </button>
  );
}

/** A round swatch to choose a colour by, ringed and ticked when chosen, the tick in `on`. Without a
 * colour, a hollow circle, for a default with none of its own; or, `rainbow`, every hue, for a colour
 * still to make. */
export function Swatch({
  label,
  color,
  on,
  rainbow = false,
  chosen,
  onChoose,
}: {
  label: string;
  color?: string;
  on?: string;
  rainbow?: boolean;
  chosen: boolean;
  onChoose(): void;
}) {
  return (
    <button type="button" className="color-choice" aria-label={label} aria-pressed={chosen} title={label} onClick={onChoose}>
      <span className={`swatch${color ? "" : rainbow ? " rainbow" : " none"}`} style={{ "--swatch": color, "--swatch-on": on }}>
        {chosen && <Icon of={CheckIcon} size={16} />}
      </span>
    </button>
  );
}

/** The square box of a checklist item, ticked or not; what is clicked is the item around it. */
export function Box({ checked }: { checked: boolean }) {
  return (
    <span className="box" data-checked={checked}>
      <Icon of={CheckIcon} size={12} width={3} />
    </span>
  );
}

/** The whole row toggles it, as in the system's settings. */
export function Switch({ label, sub, on, onChange }: { label: string; sub: string; on: boolean; onChange(on: boolean): void }) {
  return (
    <label className="row">
      <div className="row-inner">
        <span className="folder-main">
          <span>{label}</span>
          <span className="folder-sub full">{sub}</span>
        </span>
        <Toggle.Root className="switch" checked={on} onCheckedChange={onChange}>
          <Toggle.Thumb />
        </Toggle.Root>
      </div>
    </label>
  );
}

/** iOS-style segmented control: one of a few, picked as radio buttons are, arrow keys too. `label`
 * names the choice for screen readers. */
export function Seg<T extends string>({
  label,
  items,
  value,
  onChange,
  small,
}: {
  label: string;
  items: [T, string, LucideIcon?][];
  value: T;
  onChange(v: T): void;
  small?: boolean;
}) {
  return (
    <RadioGroup.Root
      className={`seg${small ? " sm" : ""}`}
      style={{ "--n": items.length, "--i": items.findIndex(([v]) => v === value) }}
      aria-label={label}
      value={value}
      onValueChange={(v) => onChange(v as T)}
    >
      {items.map(([v, label, icon]) => (
        <RadioGroup.Item key={v} value={v}>
          {icon && <Icon of={icon} size={16} />}
          {label}
        </RadioGroup.Item>
      ))}
    </RadioGroup.Root>
  );
}

/** One of `items`, too many for a `Seg`, from a menu like the "…" button's, the current one ticked. A
 * `row` of settings, `label` on the left and the value on the right; or a box like a text field's,
 * labelled above by the field. */
export function Select<T extends string>({
  label,
  row,
  items,
  value,
  onChange,
  disabled,
}: {
  label: string;
  row?: boolean;
  items: [T, string][];
  value: T;
  onChange(v: T): void;
  disabled?: boolean;
}) {
  const root = useContext(RootContext);
  const [open, setOpen] = useState(false);
  useBack("dialog", open ? () => setOpen(false) : null);
  const text = items.find(([v]) => v === value)![1];
  const chevron = <Icon of={ChevronsUpDown} size={16} />;
  return (
    <Choice.Root value={value} onValueChange={(v) => onChange(v as T)} open={open} onOpenChange={setOpen} disabled={disabled}>
      {row ? (
        <Choice.Trigger className="row pick" aria-label={label}>
          <div className="row-inner">
            {/* Its name already: what is read out after it is the value. */}
            <span className="row-name" aria-hidden>{label}</span>
            <span className="row-value long">{text}</span>
            {chevron}
          </div>
        </Choice.Trigger>
      ) : (
        <Choice.Trigger className="select" aria-label={label}>
          {text}
          {chevron}
        </Choice.Trigger>
      )}
      <Choice.Portal container={root}>
        <Choice.Content className="menu choose" position="popper" align={row ? "end" : "start"} sideOffset={4}>
          <Choice.Viewport>
            {items.map(([v, text]) => (
              <Choice.Item key={v} value={v} className="menu-item">
                <Choice.ItemText>{text}</Choice.ItemText>
                <Choice.ItemIndicator className="tick">
                  <Icon of={CheckIcon} size={18} />
                </Choice.ItemIndicator>
              </Choice.Item>
            ))}
          </Choice.Viewport>
        </Choice.Content>
      </Choice.Portal>
    </Choice.Root>
  );
}

/** A row whose menu ticks any number of `items` and stays open meanwhile; `text` says what is ticked.
 * First is `none`, ticked while nothing else is, which takes the rest off; last may be `more`, an action
 * of its own that closes the menu, such as adding one not listed. */
export function MultiSelect<T extends string>({
  label,
  text,
  items,
  value,
  onChange,
  none,
  more,
}: {
  label: string;
  text: string;
  items: [T, string][];
  value: T[];
  onChange(v: T[]): void;
  none: string;
  more?: [string, () => void];
}) {
  const root = useContext(RootContext);
  const [open, setOpen] = useState(false);
  useBack("dialog", open ? () => setOpen(false) : null);
  const tick = (
    <DropdownMenu.ItemIndicator className="tick">
      <Icon of={CheckIcon} size={18} />
    </DropdownMenu.ItemIndicator>
  );
  return (
    <DropdownMenu.Root open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger className="row pick" aria-label={label}>
        <div className="row-inner">
          <span className="row-name" aria-hidden>
            {label}
          </span>
          <span className="row-value long">{text}</span>
          <Icon of={ChevronsUpDown} size={16} />
        </div>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal container={root}>
        <DropdownMenu.Content className="menu choose multi" align="end" sideOffset={4}>
          <DropdownMenu.CheckboxItem className="menu-item" checked={value.length === 0} onSelect={(e) => e.preventDefault()} onCheckedChange={() => onChange([])}>
            {none}
            {tick}
          </DropdownMenu.CheckboxItem>
          {items.map(([v, text]) => (
            <DropdownMenu.CheckboxItem
              key={v}
              className="menu-item"
              checked={value.includes(v)}
              onSelect={(e) => e.preventDefault()}
              onCheckedChange={(on) => onChange(on ? [...value, v] : value.filter((x) => x !== v))}
            >
              {text}
              {tick}
            </DropdownMenu.CheckboxItem>
          ))}
          {more && (
            <DropdownMenu.Item className="menu-item" onSelect={more[1]}>
              {more[0]}
            </DropdownMenu.Item>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Typed out of range, it takes the nearest bound rather than refusing it. */
export function Stepper({ label, value, min, max, onChange }: { label: string; value: number; min: number; max: number; onChange(n: number): void }) {
  // While typing, what is typed, empty say.
  const [typed, setTyped] = useState<string | null>(null);
  const digits = String(max).length;
  return (
    <span className="stepper">
      <Tip label="减少">
        <button disabled={value <= min} onClick={() => onChange(value - 1)}>
          <Icon of={Minus} size={16} />
        </button>
      </Tip>
      <input
        inputMode="numeric"
        aria-label={label}
        style={{ width: `${digits + 1}ch` }}
        value={typed ?? value}
        onChange={(e) => {
          const entered = e.target.value.replace(/\D/g, "");
          // Too many is the most there can be at once; too few may be the start of more.
          setTyped(Number(entered) > max ? String(max) : entered);
          if (entered) onChange(Math.min(max, Math.max(min, Number(entered))));
        }}
        onBlur={() => setTyped(null)}
      />
      <Tip label="增加">
        <button disabled={value >= max} onClick={() => onChange(value + 1)}>
          <Icon of={Plus} size={16} />
        </button>
      </Tip>
    </span>
  );
}

/** Text to copy, such as a token or a command, with the button that copies it; `children` go next to
 * the label. */
export function Copyable({ label, hint, text, children }: { label: string; hint?: string; text: string; children?: ReactNode }) {
  const copy = useCopy();
  return (
    <div className="field">
      <div className="copy-head">
        <span>
          {label} {hint && <span className="hint">{hint}</span>}
        </span>
        {children}
      </div>
      <div className="secret">
        <span>{text}</span>
        <Tip label={`复制${label}`}>
          <button className="icon-btn sm" onClick={() => copy(text, label)}>
            <Icon of={Copy} size={18} />
          </button>
        </Tip>
      </div>
    </div>
  );
}
