// Descriptions in Markdown. Raw HTML is not rendered. Checklist items can be ticked in place.

import { Check } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Icon } from "./icons";

const TASK = /^(\s*(?:[-*+]|\d+[.)])\s+\[)([ xX])(\])/;

/** The source with the checklist item on `line` (1-based) ticked or unticked. */
export function toggleLine(source: string, line: number): string {
  const lines = source.split("\n");
  lines[line - 1] = lines[line - 1].replace(TASK, (_, open, mark, close) => `${open}${mark === " " ? "x" : " "}${close}`);
  return lines.join("\n");
}

export function Description({ source, onChange }: { source: string; onChange(source: string): void }) {
  if (!source.trim()) return <p className="muted">暂无描述</p>;
  return (
    <Markdown
      remarkPlugins={[remarkGfm]}
      components={{
        a: ({ node, ...props }) => <a {...props} target="_blank" rel="noreferrer" />,
        input: ({ checked }) => (
          <span className="box" data-checked={checked}>
            <Icon of={Check} size={12} width={3} />
          </span>
        ),
        li: ({ node, className, children }) => {
          if (!className?.includes("task-list-item")) return <li>{children}</li>;
          const checked = node!.children.some((c) => c.type === "element" && c.tagName === "input" && c.properties.checked);
          const line = node!.position!.start.line;
          return (
            <li
              className={`ck${checked ? " on" : ""}`}
              onClick={(e) => {
                e.stopPropagation();
                if (!(e.target as HTMLElement).closest("a")) onChange(toggleLine(source, line));
              }}
            >
              {children}
            </li>
          );
        },
      }}
    >
      {source}
    </Markdown>
  );
}
