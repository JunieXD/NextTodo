import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { Description } from "./Markdown";

const shown = (source: string) => renderToStaticMarkup(createElement(Description, { source, attachments: [], onChange() {}, onAttachment() {} }));

test("a line break typed in a description is one shown, in lists and checklists too, not in code", () => {
  expect(shown("买菜\n做饭")).toBe("<p>买菜<br/>\n做饭</p>");
  expect(shown("买菜\r做饭")).toBe("<p>买菜<br/>\n做饭</p>");
  expect(shown("- 鸡蛋\n  两盒")).toContain("<li>鸡蛋<br/>\n两盒</li>");
  expect(shown("- [ ] 签合同\n  带上身份证")).toContain("签合同<br/>\n带上身份证</li>");
  expect(shown("```\na\nb\n```")).toContain("<code>a\nb\n</code>");
  // A blank line still starts a paragraph.
  expect(shown("一\n\n二")).toBe("<p>一</p>\n<p>二</p>");
});
