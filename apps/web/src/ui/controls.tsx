import { Check as CheckIcon, type LucideIcon } from "lucide-react";
import type { Status } from "../domain/board";
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

/** iOS-style segmented control. */
export function Seg<T extends string>({ items, value, onChange, small }: { items: [T, string, LucideIcon?][]; value: T; onChange(v: T): void; small?: boolean }) {
  return (
    <div className={`seg${small ? " sm" : ""}`} role="tablist">
      {items.map(([v, label, icon]) => (
        <button key={v} role="tab" aria-selected={v === value} className={v === value ? "on" : ""} onClick={() => onChange(v)}>
          {icon && <Icon of={icon} size={16} />}
          {label}
        </button>
      ))}
    </div>
  );
}
