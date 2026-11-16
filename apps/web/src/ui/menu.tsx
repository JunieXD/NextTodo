// Menus of actions, alike whether a "…" button or a right click opens them; and the browser's own menu
// only where it helps, on text.

import { Ellipsis, type LucideIcon } from "lucide-react";
import { ContextMenu as Context, DropdownMenu } from "radix-ui";
import { type ReactElement, type ReactNode, useContext, useRef, useState } from "react";
import { useBack } from "./back";
import { RootContext, Tip } from "./feedback";
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

/** A button opening a menu of `items`: by default a "…" one, which `label` names. */
export function MenuButton({
  label,
  items,
  className = "icon-btn",
  align = "end",
  children = <Icon of={Ellipsis} />,
}: {
  label?: string;
  items: MenuItem[];
  className?: string;
  align?: "start" | "end";
  children?: ReactNode;
}) {
  const root = useContext(RootContext);
  const [menu, setMenu] = useState(false);
  // A finger come down on the button, to open the menu once the tap is done.
  const tap = useRef(false);
  useBack("dialog", menu ? () => setMenu(false) : null);
  const trigger = (
    <DropdownMenu.Trigger
      className={className}
      // Radix opens the menu as the button goes down. A finger's opens it once the tap is done, as the
      // system's menus do: the press shows, and a scroll or a long press begun on it opens nothing. A
      // click with no pointer behind it, from a screen reader say, opens it too.
      onPointerDown={(e) => {
        tap.current = e.pointerType !== "mouse";
        if (tap.current) e.preventDefault();
      }}
      onClick={(e) => {
        e.stopPropagation();
        if (tap.current || !(e.nativeEvent as PointerEvent).pointerType) setMenu(true);
        tap.current = false;
      }}
    >
      {children}
    </DropdownMenu.Trigger>
  );
  return (
    <DropdownMenu.Root open={menu} onOpenChange={setMenu}>
      {label ? <Tip label={label}>{trigger}</Tip> : trigger}
      <DropdownMenu.Portal container={root}>
        <DropdownMenu.Content
          className="menu"
          align={align}
          sideOffset={4}
          // A tap on the button while the menu is open comes down outside it: it closes the menu, and
          // its click opens nothing.
          onPointerDownOutside={() => (tap.current = false)}
          // Rendered elsewhere, its clicks still reach what holds the button in React, such as a row
          // that opens on a click.
          onClick={(e) => e.stopPropagation()}
        >
          {items.map((item) => (
            <DropdownMenu.Item key={item.label} className={itemClass(item)} onSelect={item.onSelect}>
              <Icon of={item.icon} />
              {item.label}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
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
