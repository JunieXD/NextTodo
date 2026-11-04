// Menus of actions, alike whether a "…" button or a right click opens them; and the browser's own menu
// only where it helps, on text.

import type { LucideIcon } from "lucide-react";
import { ContextMenu as Context } from "radix-ui";
import { type ReactElement, useContext, useRef } from "react";
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
export function ContextMenu({ items, children }: { items: MenuItem[]; children: ReactElement }) {
  const root = useContext(RootContext);
  const touched = useRef(false);
  return (
    <Context.Root>
      {/* Radix opens the menu on a long press itself, and on the browser's menu that comes of one:
          kept from both, as an event handled already. */}
      <Context.Trigger
        asChild
        onPointerDown={(e) => {
          touched.current = e.pointerType !== "mouse";
          if (touched.current) e.preventDefault();
        }}
        onContextMenu={(e) => {
          if (touched.current) e.preventDefault();
        }}
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
