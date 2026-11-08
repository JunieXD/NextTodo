import { Check as CheckIcon, ChevronsUpDown, type LucideIcon } from "lucide-react";
import { Select as Choice, RadioGroup } from "radix-ui";
import { useContext, useState } from "react";
import type { Status } from "../domain/board";
import { useBack } from "./back";
import { RootContext } from "./feedback";
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

/** The square box of a checklist item, ticked or not; what is clicked is the item around it. */
export function Box({ checked }: { checked: boolean }) {
  return (
    <span className="box" data-checked={checked}>
      <Icon of={CheckIcon} size={12} width={3} />
    </span>
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
    <RadioGroup.Root className={`seg${small ? " sm" : ""}`} aria-label={label} value={value} onValueChange={(v) => onChange(v as T)}>
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
            <span aria-hidden>{label}</span>
            <span className="row-value">{text}</span>
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
