// Markdown: descriptions, whose checklist items can be ticked in place, and whose references to the
// task's attachments are tags that open them (docs/ui.md#附件). Raw HTML is not rendered. Loaded when
// first shown, being large and not needed to start (`lazyMarkdown`).

import { File as FileIcon, Image as ImageIcon } from "lucide-react";
import type { ReactNode } from "react";
import Markdown, { type Components, defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Attachment } from "../api/types/Attachment";
import { isPicture } from "../attach";
import { Box } from "./controls";
import { Icon } from "./icons";

const TASK = /^(\s*(?:[-*+]|\d+[.)])\s+\[)([ xX])(\])/;

/** The source with the checklist item on `line` (1-based) ticked or unticked. */
export function toggleLine(source: string, line: number): string {
  const lines = source.split("\n");
  lines[line - 1] = lines[line - 1].replace(TASK, (_, open, mark, close) => `${open}${mark === " " ? "x" : " "}${close}`);
  return lines.join("\n");
}

/** Of the syntax tree, what turning line breaks into breaks needs. */
type Node = { type: string; value?: string; children?: Node[] };

/** Line endings, as Markdown has them. */
const LINE = /\r\n?|\n/;

/** A line break typed is one shown, as in GitHub's comments: Markdown takes a single one for a space. */
function breaks() {
  const within = (node: Node): Node[] => {
    if (node.type === "text" && LINE.test(node.value!)) {
      return node.value!.split(LINE).flatMap((line, i) => [...(i ? [{ type: "break" }] : []), { type: "text", value: line }]);
    }
    if (node.children) node.children = node.children.flatMap(within);
    return [node];
  };
  return (tree: Node) => void within(tree);
}

const REFERENCE = "attachment:";

/** Links as react-markdown allows them, `http`, `https`, `mailto` and the like, and references. */
const urlTransform = (url: string) => (url.startsWith(REFERENCE) ? url : defaultUrlTransform(url));

/** At most 16 characters of a name, shortened. */
const short = (name: string) => ([...name].length > 16 ? `${[...name].slice(0, 16).join("")}…` : name);

/** A reference: the attachment's name as it is now, which opens it; greyed out, with the name it was
 * written with, when it is not one of the task's. */
function Reference({ a, written, onOpen }: { a: Attachment | undefined; written: ReactNode; onOpen(a: Attachment): void }) {
  if (!a) {
    return (
      <span className="ref-chip gone">
        <Icon of={FileIcon} size={14} />
        <span>{written}</span>
      </span>
    );
  }
  return (
    <button type="button" className="ref-chip" title={a.name} onClick={() => onOpen(a)}>
      <Icon of={isPicture(a.type) ? ImageIcon : FileIcon} size={14} />
      <span>{short(a.name)}</span>
    </button>
  );
}

export function Description({
  source,
  attachments,
  onChange,
  onAttachment,
}: {
  source: string;
  /** The task's. */
  attachments: Attachment[];
  onChange(source: string): void;
  onAttachment(a: Attachment): void;
}) {
  if (!source.trim()) return <p className="muted">暂无描述</p>;
  // Links open outside the app.
  const link: Components["a"] = ({ node, href, children, ...props }) =>
    href?.startsWith(REFERENCE) ? (
      <Reference a={attachments.find((a) => a.id === href.slice(REFERENCE.length))} written={children} onOpen={onAttachment} />
    ) : (
      <a {...props} href={href} target="_blank" rel="noreferrer">
        {children}
      </a>
    );
  return (
    <Markdown
      remarkPlugins={[remarkGfm, breaks]}
      urlTransform={urlTransform}
      components={{
        a: link,
        input: ({ checked }) => <Box checked={!!checked} />,
        li: ({ node, className, children }) => {
          if (!className?.includes("task-list-item")) return <li>{children}</li>;
          const checked = node!.children.some((c) => c.type === "element" && c.tagName === "input" && c.properties.checked);
          const line = node!.position!.start.line;
          return (
            <li
              className={`ck${checked ? " on" : ""}`}
              onClick={(e) => {
                e.stopPropagation();
                if (!(e.target as HTMLElement).closest("a, .ref-chip")) onChange(toggleLine(source, line));
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
