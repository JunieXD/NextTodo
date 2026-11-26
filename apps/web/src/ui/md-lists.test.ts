import { expect, test } from "vitest";
import { type Edit, type ListKind, continueList, toggleList } from "./md-lists";

/** `text` with the caret, or the two ends of what is selected, at each `|`. */
const parse = (marked: string): [string, number, number] => {
  const [a, b = a] = [...marked.matchAll(/\|/g)].map((m, i) => m.index - i);
  return [marked.replaceAll("|", ""), a, b];
};
const show = (text: string, e: Edit) => {
  const out = text.slice(0, e.from) + e.text + text.slice(e.to);
  const [a, b] = e.select;
  return a === b ? out.slice(0, a) + "|" + out.slice(a) : out.slice(0, a) + "|" + out.slice(a, b) + "|" + out.slice(b);
};
const toggled = (marked: string, kind: ListKind) => {
  const [text, from, to] = parse(marked);
  return show(text, toggleList(text, from, to, kind));
};
const entered = (marked: string) => {
  const [text, at] = parse(marked);
  const e = continueList(text, at);
  return e && show(text, e);
};

test("a list button makes the caret's line an item, keeping the caret in its words, and takes it back", () => {
  expect(toggled("|", "check")).toBe("- [ ] |");
  expect(toggled("买|菜", "check")).toBe("- [ ] 买|菜");
  expect(toggled("- [ ] 买|菜", "check")).toBe("买|菜");
  // Another kind is changed to this one; done or not goes with the checkbox.
  expect(toggled("- [x] 买|菜", "bullet")).toBe("- 买|菜");
  expect(toggled("- 买菜|", "number")).toBe("1. 买菜|");
  expect(toggled("  * 鸡蛋|", "check")).toBe("  - [ ] 鸡蛋|");
  // In the marker, the caret goes after it.
  expect(toggled("-| 买菜", "check")).toBe("- [ ] |买菜");
  // Only that line, the first one too when it is blank, and one holding a line separator.
  expect(toggled("早饭\n午|饭\n晚饭", "bullet")).toBe("早饭\n- 午|饭\n晚饭");
  expect(toggled("|\n买菜", "check")).toBe("- [ ] |\n买菜");
  expect(toggled("早\u2028饭|", "bullet")).toBe("- 早\u2028饭|");
  // Checklist items as GFM reads them.
  expect(toggled("-  [x] 买|菜", "check")).toBe("买|菜");
  expect(toggled("1. [x] 买菜|", "number")).toBe("1. 买菜|");
});

test("on lines selected, a list button makes items of them all, numbered, and leaves blank lines be", () => {
  expect(toggled("|一\n二\n\n三|", "number")).toBe("|1. 一\n2. 二\n\n3. 三|");
  expect(toggled("|1. 一\n2. 二|", "number")).toBe("|一\n二|");
  // Some items of it already: all of them are, those ticked still so.
  expect(toggled("一\n二|\n- [x] 三|", "check")).toBe("一\n|- [ ] 二\n- [x] 三|");
  // Whole lines, up to the next one's start, are just those.
  expect(toggled("|一\n二\n|三", "bullet")).toBe("|- 一\n- 二|\n三");
});

test("Enter at the end of an item starts the next, and on an empty one ends the list", () => {
  expect(entered("- [x] 买菜|")).toBe("- [x] 买菜\n- [ ] |");
  expect(entered("9. 买菜|")).toBe("9. 买菜\n10. |");
  expect(entered("早饭\n  * 鸡蛋|\n晚饭")).toBe("早饭\n  * 鸡蛋\n  * |\n晚饭");
  expect(entered("- 买菜\n- [ ] |")).toBe("- 买菜\n|");
  expect(entered("- [ ]|")).toBe("|");
  expect(entered("买菜\n-|")).toBe("买菜\n|");
  expect(entered("1.|")).toBe("|");
  expect(entered("1. [x] 买菜|")).toBe("1. [x] 买菜\n2. [ ] |");
  // Elsewhere, only a line break.
  expect(entered("买菜|")).toBeNull();
  expect(entered("- 买|菜")).toBeNull();
  // In a fenced code block, only a line break; after one, the list goes on.
  expect(entered("```yaml\n- 参数|")).toBeNull();
  expect(entered("```\na\n```\n- 买菜|")).toBe("```\na\n```\n- 买菜\n- |");
  // Closed only by a fence of its kind, at least as long.
  expect(entered("````\n```\n- 参数|")).toBeNull();
  expect(entered("```\n~~~\n- 参数|")).toBeNull();
  expect(entered("~~~~\n~~~~~\n- 买菜|")).toBe("~~~~\n~~~~~\n- 买菜\n- |");
  // Code in a line is no fence; a closing one has only spaces after it.
  expect(entered("```npm install```\n\n- 改配置|")).toBe("```npm install```\n\n- 改配置\n- |");
  expect(entered("```\n```\u00a0\n- 参数|")).toBeNull();
});
