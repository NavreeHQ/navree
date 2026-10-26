// Checks docs pages without building the site, so several writers can check
// their own pages at once:
//   node scripts/check-docs.ts content/docs/basics/validation.mdx [...more]
//   node scripts/check-docs.ts            (every page)
// For each page: front-matter has `title` and `description`; the MDX
// compiles; only the site's MDX components are used; code fences use a
// language the highlighter knows; `/docs/...` links point at real pages;
// and the "This page is being written" stub callout is gone.
import { compile } from "@mdx-js/mdx";
import fs from "node:fs";
import path from "node:path";
import matter from "gray-matter";
import remarkGfm from "remark-gfm";

import { sections } from "../content/docs/manifest.ts";

const COMPONENTS = new Set(["Badge", "Callout", "CodeGroup", "Diagram", "FileTree", "LinkCard", "LinkCards", "Step", "Steps", "Tab", "Tabs"]);
const LANGUAGES = new Set([
  "rust", "toml", "shellscript", "typescript", "tsx", "json", "sql", "http", "html", "jinja", "yaml", "dotenv",
  "sh", "bash", "shell", "zsh", "console", "ts", "js", "javascript", "env", "text", "txt", "plaintext", "diff",
]);

const root = path.resolve(import.meta.dirname, "..");
const docsDir = path.join(root, "content", "docs");
const pages = new Set(sections.flatMap((s) => s.pages.map((p) => `/docs/${s.slug}/${p}/`)));

const files = process.argv.length > 2
  ? process.argv.slice(2).map((f) => path.resolve(f))
  : sections.flatMap((s) => s.pages.map((p) => path.join(docsDir, s.slug, `${p}.mdx`)));

let problems = 0;
const report = (file: string, msg: string) => {
  problems++;
  console.log(`${path.relative(root, file)}: ${msg}`);
};

for (const file of files) {
  if (!fs.existsSync(file)) {
    report(file, "file not found");
    continue;
  }
  const { data, content } = matter(fs.readFileSync(file, "utf8"));
  if (!data.title) report(file, "front-matter: missing title");
  if (!data.description) report(file, "front-matter: missing description");
  if (/This page is being written/.test(content)) report(file, "still contains the stub callout");

  try {
    await compile(content, { remarkPlugins: [remarkGfm] });
  } catch (error) {
    const e = error as { line?: number; message: string };
    report(file, `MDX does not compile${e.line ? ` (line ${e.line})` : ""}: ${e.message.split("\n")[0]}`);
  }

  // Outside code fences: components and links.
  const prose = content.replace(/^(```|~~~)[\s\S]*?^\1/gm, "").replace(/`[^`\n]*`/g, "");
  for (const [, name] of prose.matchAll(/<([A-Z][A-Za-z0-9]*)/g)) {
    if (!COMPONENTS.has(name)) report(file, `unknown component <${name}>`);
  }
  for (const [, href] of prose.matchAll(/\]\((\/docs\/[^)#\s]*)/g)) {
    const normal = href.endsWith("/") ? href : `${href}/`;
    if (!pages.has(normal)) report(file, `link to a page that doesn't exist: ${href}`);
  }
  for (const [, href] of prose.matchAll(/href="(\/docs\/[^"#]*)/g)) {
    const normal = href.endsWith("/") ? href : `${href}/`;
    if (!pages.has(normal)) report(file, `link to a page that doesn't exist: ${href}`);
  }
  for (const [, lang] of content.matchAll(/^(?:```|~~~)([A-Za-z0-9_+-]*)/gm)) {
    if (lang && !LANGUAGES.has(lang)) report(file, `code fence language not highlighted: ${lang}`);
  }
}

console.log(problems === 0 ? `OK: ${files.length} page(s) checked` : `${problems} problem(s) in ${files.length} page(s)`);
process.exit(problems === 0 ? 0 : 1);
