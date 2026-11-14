// Menus of actions, alike whether a "…" button or a right click opens them; and the browser's own menu
// only where it helps, on text.

import type { LucideIcon } from "lucide-react";
import { ContextMenu as Context } from "radix-ui";
import { type ReactElement, useContext, useState } from "react";
import { RootContext } from "./feedback";
import { Icon } from "./icons";

export interface MenuItem {
  icon: LucideIcon;
  label: string;
  danger?: boolean;
  onSelect(): void;
}

export const itemClass = (item: MenuItem) => `menu-item${item.danger ? " danger-text" : ""}`;

/** Right-clicking `children` opens a menu of `items`. Only with a mouse, also on a computer with a
 * touch screen: a long press of a finger or pen picks a row up to drag it. */
/** `off`: right-clicking does what it does elsewhere, as while tasks are being chosen. */
export function ContextMenu({ items, off, children }: { items: MenuItem[]; off?: boolean; children: ReactElement }) {
  const root = useContext(RootContext);
  // Radix opens the menu on a long press itself, and on the browser's menu that comes of one. Off
  // from a touch on, which also stops the long press begun (Radix 2.4); on again for a mouse, or for
  // the menu key.
  const [touched, setTouched] = useState(false);
  return (
    <Context.Root>
      <Context.Trigger
        asChild
        disabled={touched || off}
        onPointerDown={(e) => setTouched(e.pointerType !== "mouse")}
        onKeyDown={() => setTouched(false)}
      >
        {children}
      </Context.Trigger>
      <Context.Portal container={root}>
        <Context.Content className="menu">
          {items.map((item) => (
            <Context.Item key={item.label} className={itemClass(item)} onSelect={item.onSelect}>
              <Icon of={item.icon} />
              {item.label}
            </Context.Item>
          ))}
        </Context.Content>
      </Context.Portal>
    </Context.Root>
  );
}

/** Keeps the browser's menu to what it is good for, copying and pasting: in text fields, on selected
 * text and on links. Elsewhere there is the app's menu or none. */
export function limitBrowserMenu() {
  addEventListener("contextmenu", (e) => {
    const target = e.target as Element;
    const selection = getSelection();
    if (target.closest("input, textarea, [contenteditable], a[href]") || (selection && !selection.isCollapsed && selection.containsNode(target, true))) return;
    e.preventDefault();
  });
}
