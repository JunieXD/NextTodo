import { useKept } from "../kept";

/** Whether a key goes to an input method still composing, as Enter confirming the letters typed. WebKit,
 * Safari's and the macOS app's, ends the composition before that Enter comes, so `isComposing` is
 * false then; its key code, 229, still tells. */
export const composing = (e: React.KeyboardEvent) => e.nativeEvent.isComposing || e.keyCode === 229;

const SEND_KEY = "nexttodo.enterSends";

/** Whether Enter sends AI organizing's text, of many lines, and Shift + Enter starts a line, as set on
 * this device; else Enter starts one and only Ctrl or ⌘ + Enter sends (docs/ui.md#设置). */
export function useEnterSends(): [boolean, (on: boolean) => void] {
  const [kept, keep] = useKept(SEND_KEY);
  return [kept === "on", (on) => keep(on ? "on" : "off")];
}

/** Whether `e` is the key that sends a text of many lines: Ctrl or ⌘ + Enter, and Enter alone where it
 * is set to. */
export const isSendKey = (e: React.KeyboardEvent, enterSends: boolean) =>
  e.key === "Enter" && !composing(e) && (e.ctrlKey || e.metaKey || (enterSends && !e.shiftKey && !e.altKey));
