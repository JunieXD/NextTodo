// Finds broken links into the docs, as this repository writes them: paths from the repository root
// (`docs/ui.md#颜色`, in comments, README and GitHub URLs) anywhere, and inline links `[text](path#anchor)`
// in the Markdown outside the site (whose build checks its pages, not their anchors). Sections are ATX
// headings, named as GitHub names them; code in ``` fences is not looked at. Reference links, <…>
// destinations, setext headings and HTML anchors aren't used here, and aren't understood.
// Looks at `git ls-files` with the pathspecs given after `--`, untracked files too, as in the working
// tree; prints what is broken and exits 1. Run by check.sh and .githooks/pre-commit.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, normalize } from "node:path";

// Kept as they are: inside a migration's CREATE TABLE, a comment is part of the schema stored in every
// database (crates/server/src/db.rs, `ours`), which migrations/schema.txt records.
const KEPT = new Set(["crates/server/migrations/014_attachments.sql", "crates/server/migrations/schema.txt"].map((f) => `${f} docs/local.md#附件`));

process.chdir(join(import.meta.dirname, ".."));
const files = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard", ...process.argv.slice(2)], { encoding: "utf8" })
  .split("\0")
  // Not this file, whose KEPT names links that don't work, nor the made-up paths in test data; nor
  // files deleted and not committed yet.
  .filter((f) => f && f !== "docs/links.mjs" && !f.startsWith("fixtures/") && existsSync(f));

/** Lines outside ``` fences in Markdown, as [line, number]. */
function* prose(text, markdown) {
  let fence = "";
  for (const [i, line] of text.split("\n").entries()) {
    const ticks = markdown ? /^ {0,3}(`{3,})([^`]*)$/.exec(line) : null;
    if (!fence) {
      if (ticks) fence = ticks[1];
      else yield [line, i + 1];
    } else if (ticks && ticks[1].length >= fence.length && !ticks[2].trim()) fence = "";
  }
}

// GitHub's anchors (github-slugger): the heading's text in lower case, punctuation dropped, spaces as
// hyphens; one already taken gets -1, -2, … until it is free.
const anchors = new Map();
function anchorsOf(path) {
  if (!anchors.has(path)) {
    const taken = new Map();
    for (const [line] of prose(readFileSync(path, "utf8"), true)) {
      const heading = /^#{1,6} (.*)$/.exec(line);
      if (!heading) continue;
      const base = heading[1].replace(/\[([^\]]*)\]\([^)]*\)/g, "$1").trim().toLowerCase().replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "").replace(/ /g, "-");
      let slug = base;
      while (taken.has(slug)) {
        taken.set(base, taken.get(base) + 1);
        slug = `${base}-${taken.get(base)}`;
      }
      taken.set(slug, 0);
    }
    anchors.set(path, taken);
  }
  return anchors.get(path);
}

const decode = (s) => {
  try {
    return decodeURIComponent(s);
  } catch {
    return null;
  }
};

const ANCHOR = String.raw`[^\s)\]\x60"'<>|*，。、；：（）,]+`;
const FROM_ROOT = new RegExp(String.raw`(?<![\w-])(docs/(?:[\w.-]+/)*[\w.-]+\.md)(?!\w)(?:#(${ANCHOR}))?`, "gu");
const MARKDOWN = /\]\(([^)\s]+)\)/g;
const broken = [];
for (const file of files) {
  const bytes = readFileSync(file);
  if (bytes.includes(0)) continue;
  const markdown = file.endsWith(".md") && !file.startsWith("apps/site/");
  for (const [line, number] of prose(bytes.toString("utf8"), markdown)) {
    const links = [];
    const relatives = [];
    if (markdown) {
      for (const m of line.matchAll(MARKDOWN)) {
        const target = m[1];
        if (/^[a-z]+:|^\//.test(target)) continue;
        const [path, anchor] = target.split("#", 2);
        const relative = decode(path.split("?")[0]);
        links.push([target, relative === null ? null : relative ? normalize(join(dirname(file), relative)) : file, anchor]);
        relatives.push([m.index, m.index + m[0].length]);
      }
    }
    for (const m of line.matchAll(FROM_ROOT)) {
      if (!relatives.some(([start, end]) => start < m.index && m.index < end)) links.push([m[0], m[1], m[2]?.replace(/[.:]+$/, "")]);
    }
    for (const [written, path, raw] of links) {
      if (KEPT.has(`${file} ${written}`)) continue;
      const anchor = raw === undefined ? undefined : decode(raw);
      const found = path !== null && anchor !== null && existsSync(path) && (!anchor || !path.endsWith(".md") || anchorsOf(path).has(anchor));
      if (!found) broken.push(`${file}:${number}: ${written}`);
    }
  }
}
if (broken.length) {
  console.error(`这些链接指向的文件或章节不存在：\n${[...new Set(broken)].join("\n")}`);
  process.exit(1);
}
