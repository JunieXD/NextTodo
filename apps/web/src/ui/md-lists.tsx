// Writing lists in a description's Markdown (docs/ui.md#其他): the buttons below the box, Enter going
// on with the list a line is in, and an undo of the box's own.

import { List, ListOrdered, type LucideIcon, SquareCheck, Undo2 } from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { Tip } from "./feedback";
import { Icon } from "./icons";

export type ListKind = "check" | "bullet" | "number";

/** What to put in place of `from`–`to`, and what to select after. */
export type Edit = { from: number; to: number; text: string; select: [number, number] };

/** A line's indent, its list marker with what follows it ("" in a line of no list), and the rest. As
 * GFM reads them: a checkbox after any marker, the space after either left out at the end of the line. */
const ITEM = /^([ \t]*)((?:[-*+]|\d{1,9}[.)])(?:[ \t]+(?:\[[ xX]\](?:[ \t]+|$))?|$)|)(.*)$/s;

const kindOf = (marker: string): ListKind | null => (!marker ? null : marker.includes("[") ? "check" : /\d/.test(marker) ? "number" : "bullet");

/** Where the line `at` is on starts, in a textarea's text, whose line breaks are all `\n`. */
const lineStart = (text: string, at: number) => (at === 0 ? 0 : text.lastIndexOf("\n", at - 1) + 1);

/** Whether the line starting at `start` is in a fenced code block: one opened above and not closed by a
 * fence of the same kind, at least as long, with only spaces after it. As CommonMark has them: no
 * backtick after a backtick fence, which is code in a line. */
function inCode(text: string, start: number) {
  let open = "";
  for (const line of text.slice(0, start).split("\n")) {
    const fence = /^ {0,3}(`{3,}(?=[^`]*$)|~{3,})(.*)$/.exec(line);
    if (!fence) continue;
    if (!open) open = fence[1];
    else if (fence[1][0] === open[0] && fence[1].length >= open.length && /^[ \t]*$/.test(fence[2])) open = "";
  }
  return open !== "";
}

/** Makes the lines `from`–`to` touch items of a `kind` list, or plain lines where they are all that
 * already. Items of it stay as they are, ticked or not, but numbers go from 1; a few lines at once leave
 * the blank ones be. */
export function toggleList(text: string, from: number, to: number, kind: ListKind): Edit {
  const start = lineStart(text, from);
  // Whole lines selected end after the last one's line break, not on the next line.
  const last = to > from && text[to - 1] === "\n" ? to - 1 : to;
  const end = text.indexOf("\n", last) < 0 ? text.length : text.indexOf("\n", last);
  const lines = text
    .slice(start, end)
    .split("\n")
    .map((l) => ITEM.exec(l)!);
  const items = lines.length > 1 ? lines.filter((m) => m[0].trim()) : lines;
  const off = items.every((m) => kindOf(m[2]) === kind);
  let n = 0;
  const marker = (was: string) => (off ? "" : kind === "number" ? `${++n}. ` : kindOf(was) === kind ? was : kind === "check" ? "- [ ] " : "- ");
  const out = lines.map((m) => (items.includes(m) ? m[1] + marker(m[2]) + m[3] : m[0])).join("\n");
  if (from !== to || lines.length > 1) return { from: start, to: end, text: out, select: [start, start + out.length] };
  // The caret stays where it was in the words, after the marker if it was in that.
  const [, indent, was] = lines[0];
  const now = out.length - lines[0][3].length;
  const at = start + now + Math.max(0, from - start - indent.length - was.length);
  return { from: start, to: end, text: out, select: [at, at] };
}

/** What a line break at `at` does at the end of a list item: starts the next, or ends the list where
 * the item has nothing in it. Null elsewhere, where it is only a line break, as in a fenced code block. */
export function continueList(text: string, at: number): Edit | null {
  const start = lineStart(text, at);
  const end = text.indexOf("\n", at) < 0 ? text.length : text.indexOf("\n", at);
  const [, indent, marker, rest] = ITEM.exec(text.slice(start, at))!;
  if (!marker || text.slice(at, end).trim() || inCode(text, start)) return null;
  if (!rest.trim()) return { from: start, to: at, text: "", select: [start, start] };
  const next = `\n${indent}${marker.replace(/\[[xX]\]/, "[ ]").replace(/\d+/, (d) => String(Number(d) + 1))}`;
  return { from: at, to: at, text: next, select: [at + next.length, at + next.length] };
}

/** Makes `e` in `area` through the browser, for React to hear it as it does typing. */
function apply(area: HTMLTextAreaElement, e: Edit) {
  area.focus();
  area.setSelectionRange(e.from, e.to);
  if (e.text) document.execCommand("insertText", false, e.text);
  else if (e.from < e.to) document.execCommand("delete");
  area.setSelectionRange(...e.select);
}

type Snapshot = { text: string; select: [number, number] };

/** Typing, which goes into one step of undo while it keeps on in one place without a pause. */
const TYPING = /^(insertText|deleteContentBackward|deleteContentForward)$/;
const PAUSE = 1000;

/** The buttons below the box, which leave the focus, and the keyboard, where they are; Enter going on
 * with a list; and undo and redo of the box's own, by the button and the keys: the document's would take
 * back what was typed in other boxes too, as the title. */
export function ListButtons({ area }: { area: RefObject<HTMLTextAreaElement | null> }) {
  const [undoable, setUndoable] = useState(false);
  const act = useRef<{ edit(e: Edit): void; undo(): void; sync(): void }>(null);
  useEffect(() => {
    const a = area.current!;
    const past: Snapshot[] = [];
    const future: Snapshot[] = [];
    const now = (): Snapshot => ({ text: a.value, select: [a.selectionStart, a.selectionEnd] });
    // The text as its last change here left it: one changed from elsewhere since, as an attachment's
    // reference put in, is a step too.
    let seen = now();
    // When typing last went into the step on top, and whether it put in or took out; -Infinity when
    // the next starts a step.
    let typed = -Infinity;
    let kind = "";
    // What it was before an input method began composing, while it does.
    let composed: Snapshot | null = null;
    // While it changes the text itself, which is no step of the user's.
    let quiet = false;
    const step = (s: Snapshot) => {
      past.push(s);
      future.length = 0;
      setUndoable(true);
    };
    // Ends what an input method composes, a step once it has put something in: as it finishes, or
    // before a button or undo changes the text under it.
    const settle = () => {
      if (composed && a.value !== composed.text && past.at(-1)?.text !== composed.text) step(composed);
      composed = null;
    };
    const sync = () => {
      if (a.value === seen.text) return;
      step(seen);
      seen = now();
      typed = -Infinity;
    };
    const record = () => {
      sync();
      if (past.at(-1)?.text !== a.value) step(now());
    };
    const edit = (e: Edit) => {
      settle();
      sync();
      if (a.value.slice(e.from, e.to) === e.text) return a.setSelectionRange(...e.select);
      record();
      quiet = true;
      apply(a, e);
      quiet = false;
      seen = now();
      typed = -Infinity;
    };
    const restore = (from: Snapshot[], to: Snapshot[]) => {
      settle();
      sync();
      const s = from.pop();
      if (!s) return;
      to.push(now());
      quiet = true;
      apply(a, { from: 0, to: a.value.length, text: s.text, select: s.select });
      quiet = false;
      seen = now();
      typed = -Infinity;
      setUndoable(past.length > 0);
    };
    const before = (e: InputEvent) => {
      if (quiet) return;
      // By the line break Enter would make, which Enter only confirming what an input method was
      // composing doesn't.
      if ((e.inputType === "insertLineBreak" || e.inputType === "insertParagraph") && a.selectionStart === a.selectionEnd) {
        const next = continueList(a.value, a.selectionStart);
        if (next) {
          e.preventDefault();
          return edit(next);
        }
      }
      // What an input method composes is one step, from where it started.
      if (e.isComposing || e.inputType === "insertCompositionText") return;
      const typing = TYPING.test(e.inputType);
      const was = kind;
      kind = e.inputType.slice(0, 6);
      const moved = a.selectionStart !== seen.select[0] || a.selectionEnd !== seen.select[1];
      if (!typing || moved || e.timeStamp - typed > PAUSE || kind !== was) record();
      else sync();
      typed = typing ? e.timeStamp : -Infinity;
    };
    const composing = () => {
      sync();
      composed = now();
    };
    const composedDone = () => {
      settle();
      typed = -Infinity;
    };
    const changed = () => {
      seen = now();
    };
    const keys = (e: KeyboardEvent) => {
      const key = e.key.toLowerCase();
      const undo = (e.metaKey || e.ctrlKey) && !e.altKey && key === "z";
      if (!undo && !(e.ctrlKey && !e.metaKey && key === "y")) return;
      e.preventDefault();
      if (undo && !e.shiftKey) restore(past, future);
      else restore(future, past);
    };
    // The browser's own undo, from its menu, goes to whichever box was edited last: while this one has
    // the focus, it is this one's.
    const history = (e: InputEvent) => {
      if ((e.inputType !== "historyUndo" && e.inputType !== "historyRedo") || document.activeElement !== a) return;
      e.preventDefault();
      if (e.inputType === "historyUndo") restore(past, future);
      else restore(future, past);
    };
    act.current = { edit, undo: () => restore(past, future), sync };
    a.addEventListener("beforeinput", before);
    a.addEventListener("compositionstart", composing);
    a.addEventListener("compositionend", composedDone);
    a.addEventListener("input", changed);
    a.addEventListener("keydown", keys);
    document.addEventListener("beforeinput", history, true);
    return () => {
      a.removeEventListener("beforeinput", before);
      a.removeEventListener("compositionstart", composing);
      a.removeEventListener("compositionend", composedDone);
      a.removeEventListener("input", changed);
      a.removeEventListener("keydown", keys);
      document.removeEventListener("beforeinput", history, true);
    };
  }, [area]);
  // After whatever changed the text from elsewhere.
  useEffect(() => act.current!.sync());
  const list = (kind: ListKind) => {
    const a = area.current!;
    act.current!.edit(toggleList(a.value, a.selectionStart, a.selectionEnd, kind));
  };
  const button = (label: string, icon: LucideIcon, onClick: () => void, disabled = false) => (
    <Tip label={label}>
      <button type="button" className="icon-btn" disabled={disabled} onMouseDown={(e) => e.preventDefault()} onClick={onClick}>
        <Icon of={icon} />
      </button>
    </Tip>
  );
  return (
    <div className="md-tools">
      {button("清单", SquareCheck, () => list("check"))}
      {button("列表", List, () => list("bullet"))}
      {button("编号", ListOrdered, () => list("number"))}
      {button("撤销", Undo2, () => act.current!.undo(), !undoable)}
    </div>
  );
}
