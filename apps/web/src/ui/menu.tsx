// Menus of actions, alike whether a "…" button or a right click opens them; and the browser's own menu
// only where it helps, on text.

import type { LucideIcon } from "lucide-react";
import { ContextMenu as Context } from "radix-ui";
import { type ReactElement, useContext } from "react";
import { RootContext } from "./feedback";
import { useMedia } from "./hooks";
import { Icon } from "./icons";

export interface MenuItem {
  icon: LucideIcon;
  label: string;
  danger?: boolean;
  onSelect(): void;
}

export const itemClass = (item: MenuItem) => `menu-item${item.danger ? " danger-text" : ""}`;

/** Right-clicking `children` opens a menu of `items`. Only with a mouse: a long press on a touch
 * screen picks a row up to drag it. */
export function ContextMenu({ items, children }: { items: MenuItem[]; children: ReactElement }) {
  const root = useContext(RootContext);
  const mouse = useMedia("(pointer: fine)");
  return (
    <Context.Root>
      <Context.Trigger asChild disabled={!mouse}>
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
