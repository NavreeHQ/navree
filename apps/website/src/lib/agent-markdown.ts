/**
 * Docs for agents: every docs page as clean Markdown, `llms.txt` and
 * `llms-full.txt`.
 *
 * `scripts/agent-files.ts` writes the files into `public/` before every
 * `dev` and `build`, so they are plain static files in the export. Node
 * runs that script with the types stripped, so this module only uses
 * relative imports that end in `.ts` and erasable TypeScript.
 *
 * The conversion keeps the author's Markdown exactly as written (headings,
 * tables, lists, code fences with their language and title) and replaces
 * only the site's components with plain Markdown:
 *
 * | Component | Markdown |
 * |---|---|
 * | `<Callout type title>` | a block quote that starts with "**Tip: title**" |
 * | `<Tabs>` / `<Tab label>` | a heading per tab, one level below the section |
 * | `<CodeGroup>` | a heading per file, then its code block |
 * | `<Steps>` / `<Step title>` | numbered headings ("### 1. Install the CLI") |
 * | `<LinkCards>` / `<LinkCard>` | a list of links |
 * | `<FileTree>` | a `text` code block |
 * | `<Diagram name>` | one sentence that says what the diagram shows |
 * | `<Badge>` | plain text: "(Feature: `queue`)" |
 * | `<UiPreview example>` | the example's Rust code block |
 * | `<UiProps type>` | the type's table |
 * | `<UiAnatomy component>` | the parts as a nested list |
 * | `<UiChoose name>` | the question and a list of the answers |
 * | `<UiGrid>`, `<UiAssets>`, `<UiTokens>`, `<UiCsp>` | lists and tables of the same data |
 * | `<UiA11y>` | its Markdown |
 *
 * Links to other docs pages point at their `.md` files, and every other
 * root-relative link becomes absolute, so a page still works when it is
 * pasted into a chat. Shiki's `[!code ...]` markers are removed from code.
 */
import fs from "node:fs";
import path from "node:path";

import { createProcessor } from "@mdx-js/mdx";
import matter from "gray-matter";
import remarkGfm from "remark-gfm";

import { sections } from "../../content/docs/manifest.ts";
import { docFaq } from "../content/faq.ts";
import { keyFactsMarkdown, mainPages } from "./site-facts.ts";
import { site } from "../../site.config.ts";
import { fileTreeText } from "./file-tree-source.ts";
import {
  anatomyMarkdown,
  assetsMarkdown,
  chooseMarkdown,
  cspMarkdown,
  gridMarkdown,
  previewMarkdown,
  propsMarkdown,
  tokensMarkdown,
} from "./ui-markdown.ts";

// ------------------------------------------------------------------ pages

export type AgentPage = {
  section: string;
  sectionTitle: string;
  page: string;
  title: string;
  description: string;
  planned: boolean;
  /** `/docs/basics/validation/` */
  href: string;
  /** `/docs/basics/validation.md` */
  mdPath: string;
  /** The MDX body, front-matter removed, line endings normalized. */
  source: string;
};

/** The directory that holds the docs (`apps/website/content/docs`). */
export function docsDir(): string {
  return path.join(process.cwd(), "content", "docs");
}

/** The `.md` path of a docs page: `/docs/basics/validation.md`. */
export function markdownPath(slug: string): string {
  return `/docs/${slug}.md`;
}

/** Every page of the manifest, in sidebar order. */
export function agentPages(dir: string = docsDir()): AgentPage[] {
  const pages: AgentPage[] = [];
  for (const section of sections) {
    for (const page of section.pages) {
      const file = path.join(dir, section.slug, `${page}.mdx`);
      if (!fs.existsSync(file)) continue;
      const { data, content } = matter(fs.readFileSync(file, "utf8"));
      pages.push({
        section: section.slug,
        sectionTitle: section.title,
        page,
        title: oneLine(String(data.title ?? page)),
        description: oneLine(String(data.description ?? "")),
        planned: data.status === "planned",
        href: `/docs/${section.slug}/${page}/`,
        mdPath: markdownPath(`${section.slug}/${page}`),
        source: content.replace(/\r\n?/g, "\n"),
      });
    }
  }
  return pages;
}

const docSlugs = new Set(sections.flatMap((s) => s.pages.map((p) => `${s.slug}/${p}`)));

/** Shows an "in development" line only when the site shows planned badges. */
function statusLine(page: AgentPage): string {
  return site.showPlannedBadges && page.planned ? "\n\nStatus: in development." : "";
}

/** One docs page as Markdown: its title, its description, then the body and any questions the page answers. */
export function pageMarkdown(page: AgentPage): string {
  const head = `# ${page.title}${page.description ? `\n\n> ${page.description}` : ""}${statusLine(page)}`;
  const body = mdxToMarkdown(page.source);
  const faq = docFaq(`${page.section}/${page.page}`);
  const questions = faq
    ? `\n\n## ${faq.heading}\n\n` +
      faq.items
        .map((item) => `### ${item.question}\n\n${item.answer}${item.link ? ` [${item.link.label}](${absoluteUrl(item.link.href)})` : ""}`)
        .join("\n\n")
    : "";
  return `${head}\n\n${body}${questions}\n`;
}

// --------------------------------------------------------------- sections

/** A docs section's own page (`/docs/basics/`), published as Markdown too. */
export type AgentSection = {
  slug: string;
  title: string;
  description: string;
  /** `/docs/basics/` */
  href: string;
  /** `/docs/basics.md` */
  mdPath: string;
  /** The section's pages in order, cut into the manifest's groups; the first group may have no title. */
  groups: { title: string | null; pages: AgentPage[] }[];
};

/** Every section that has pages, in sidebar order, with its pages cut into the manifest's groups. */
export function agentSections(pages: AgentPage[]): AgentSection[] {
  return sections.flatMap((section) => {
    const own = pages.filter((page) => page.section === section.slug);
    if (!own.length) return [];
    const groups: AgentSection["groups"] = [];
    for (const page of own) {
      const start = section.groups?.find((group) => group.from === page.page);
      if (start || groups.length === 0) groups.push({ title: start?.title ?? null, pages: [] });
      groups[groups.length - 1].pages.push(page);
    }
    return [
      {
        slug: section.slug,
        title: section.title,
        description: oneLine(section.description),
        href: `/docs/${section.slug}/`,
        mdPath: markdownPath(section.slug),
        groups,
      },
    ];
  });
}

/** A section's own page as Markdown: its title, what it covers, then a link and a line for each page. */
export function sectionMarkdown(section: AgentSection): string {
  const count = section.groups.reduce((n, group) => n + group.pages.length, 0);
  const lines: string[] = [`# ${section.title}`, "", `> ${section.description}`, ""];
  lines.push(`This section of the ${site.name} documentation has ${count} ${count === 1 ? "page" : "pages"}. Each link below is the page as Markdown.`);
  for (const group of section.groups) {
    lines.push("");
    if (group.title) lines.push(`## ${group.title}`, "");
    for (const page of group.pages) {
      lines.push(`- [${page.title}](${site.url}${page.mdPath})${page.description ? `: ${page.description}` : ""}`);
    }
  }
  return `${lines.join("\n")}\n`;
}

// ------------------------------------------------------------- llms files

export type SkillLink = { name: string; summary: string; url: string };

/** A description cut to at most `max` characters at a word, with an ellipsis, for index lines. */
function cap(text: string, max = 200): string {
  const one = oneLine(text);
  if (one.length <= max) return one;
  return `${one.slice(0, max - 1).replace(/[\s,;:]+\S*$/, "")}…`;
}

/**
 * `/llms.txt` (https://llmstxt.org/): the name, a short summary in a block
 * quote, the key facts, then lists of links in sections: the main pages, one
 * line per docs page grouped by section in sidebar order (each linking the
 * page's Markdown, with the section's own page first), and an Optional
 * section with the longer files and the agent skills. Descriptions are cut to 200 characters so the file stays
 * quick to read.
 */
export function llmsTxt(pages: AgentPage[], skills: SkillLink[] = []): string {
  const out: string[] = [
    `# ${site.name}`,
    "",
    `> ${site.headline} ${site.tagline}`,
    "",
    "## Key facts",
    "",
    keyFactsMarkdown(),
    "",
    "## Main pages",
    "",
    ...mainPages.map((page) => `- [${page.title}](${site.url}${page.path}): ${page.summary}`),
  ];
  const overviews = new Map(agentSections(pages).map((section) => [section.slug, section]));
  let section = "";
  for (const page of pages) {
    if (page.section !== section) {
      section = page.section;
      out.push("", `## ${page.sectionTitle}`, "");
      const overview = overviews.get(section);
      if (overview) out.push(`- [${overview.title}: all pages](${site.url}${overview.mdPath}): ${cap(overview.description)}`);
    }
    out.push(`- [${page.title}](${site.url}${page.mdPath})${page.description ? `: ${cap(page.description)}` : ""}`);
  }
  out.push(
    "",
    "## Optional",
    "",
    `- [Every docs page in one file](${site.url}/llms-full.txt): the whole documentation as Markdown, in the order of the sidebar.`,
    `- [Sitemap](${site.url}/sitemap.xml): every page of the site with the date it last changed.`,
    `- [Changelog feed](${site.url}/changelog/feed.xml): every new feature and fix, by day, as an Atom feed.`,
  );
  if (skills.length) {
    out.push(
      `- [Agent skills index](${site.url}/skills/index.json): step-by-step instructions for coding agents, one per common task; install them with \`navree skills:install\`.`,
    );
    for (const skill of skills) out.push(`- [Skill ${skill.name}](${skill.url}): ${cap(skill.summary)}`);
  }
  return `${out.join("\n")}\n`;
}

/** `/llms-full.txt`: every page's Markdown in sidebar order, each with its URL. */
export function llmsFullTxt(pages: AgentPage[], markdown: Map<string, string>): string {
  const head = [
    `# ${site.name} documentation`,
    "",
    `> ${site.headline} ${site.tagline}`,
    "",
    "## Key facts",
    "",
    keyFactsMarkdown(),
    "",
    `Every page of ${site.url}/docs/ in one file, in the order of the sidebar. Each page starts with its title and URL. ` +
      `The index of pages is ${site.url}/llms.txt.`,
  ].join("\n");
  // Put the page's URL under its title.
  const withUrl = (md: string, href: string) => {
    const firstBreak = md.indexOf("\n");
    const title = firstBreak === -1 ? md : md.slice(0, firstBreak);
    const rest = firstBreak === -1 ? "" : md.slice(firstBreak);
    return `${title}\n\nURL: ${site.url}${href}${rest}`;
  };
  // Each section's own page comes before the section's pages.
  const overviews = new Map(agentSections(pages).map((section) => [section.slug, section]));
  const parts: string[] = [];
  let current = "";
  for (const page of pages) {
    if (page.section !== current) {
      current = page.section;
      const overview = overviews.get(current);
      if (overview) parts.push(withUrl(sectionMarkdown(overview).trimEnd(), overview.href));
    }
    parts.push(withUrl((markdown.get(page.mdPath) ?? pageMarkdown(page)).trimEnd(), page.href));
  }
  return `${head}\n\n---\n\n${parts.join("\n\n---\n\n")}\n`;
}

// -------------------------------------------------------------- docs.json

export type DocsSection = { heading: string; anchor: string; markdown: string };

export type DocsBundlePage = {
  slug: string;
  title: string;
  description: string;
  planned: boolean;
  sections: DocsSection[];
};

/** A version's docs for Navigator (`navree mcp`): `/v/<version>/docs.json`. */
export type DocsBundle = { version: string; pages: DocsBundlePage[] };

/** A heading's id on the page, as rehype-slug (github-slugger) makes it. */
function headingAnchor(heading: string, used: Map<string, number>): string {
  const text = heading
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .toLowerCase();
  const base = text.replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "").replace(/ /g, "-");
  const seen = used.get(base) ?? 0;
  used.set(base, seen + 1);
  return seen ? `${base}-${seen}` : base;
}

/**
 * Markdown split at its `##` headings (outside code blocks). The text before
 * the first heading is a section with an empty heading.
 */
export function splitSections(markdown: string): DocsSection[] {
  const sections: DocsSection[] = [];
  const used = new Map<string, number>();
  let current: DocsSection = { heading: "", anchor: "", markdown: "" };
  const lines: string[] = [];
  let fence: string | null = null;
  const flush = () => {
    current.markdown = lines.join("\n").trim();
    if (current.heading || current.markdown) sections.push(current);
    lines.length = 0;
  };
  for (const line of markdown.split("\n")) {
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker && (fence === null || (marker[0] === fence[0] && marker.length >= fence.length))) {
      fence = fence === null ? marker : null;
    }
    const heading = fence === null && !marker ? /^## +(.+?)\s*#*\s*$/.exec(line) : null;
    if (heading) {
      flush();
      current = { heading: heading[1], anchor: headingAnchor(heading[1], used), markdown: "" };
      continue;
    }
    lines.push(line);
  }
  flush();
  return sections;
}

/**
 * `/v/<version>/docs.json`: every page with its title, description, whether
 * it is planned, and its `##` sections as Markdown (components converted as
 * for the `.md` files). Navigator downloads it to answer `search_docs` and
 * `read_doc` with the docs of the Navree version an app uses.
 */
export function docsBundle(version: string, pages: AgentPage[]): DocsBundle {
  return {
    version,
    pages: pages.map((page) => ({
      slug: `${page.section}/${page.page}`,
      title: page.title,
      description: page.description,
      planned: page.planned,
      sections: splitSections(mdxToMarkdown(page.source)),
    })),
  };
}

// --------------------------------------------------------------- the tree

type Point = { line: number; column: number; offset?: number };

type MdNode = {
  type: string;
  value?: string;
  depth?: number;
  name?: string | null;
  url?: string;
  lang?: string | null;
  meta?: string | null;
  children?: MdNode[];
  attributes?: { type: string; name?: string; value?: unknown }[];
  position?: { start: Point; end: Point };
};

type Edit = { start: number; end: number; text: string };

type Ctx = {
  source: string;
  /** The depth of the last heading seen, so tabs and steps nest under it. */
  depth: number;
};

const start = (node: MdNode) => node.position?.start.offset ?? 0;
const end = (node: MdNode) => node.position?.end.offset ?? 0;

function attr(node: MdNode, name: string): string | null {
  const found = node.attributes?.find((a) => a.type === "mdxJsxAttribute" && a.name === name);
  return typeof found?.value === "string" ? found.value : null;
}

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function dedent(text: string): string {
  const lines = text.split("\n");
  const indents = lines.filter((l) => l.trim()).map((l) => l.length - l.trimStart().length);
  const min = indents.length ? Math.min(...indents) : 0;
  return min ? lines.map((l) => l.slice(Math.min(min, l.length - l.trimStart().length))).join("\n") : text;
}

/** MDX (without front-matter) to Markdown. */
export function mdxToMarkdown(mdx: string): string {
  const source = mdx.replace(/\r\n?/g, "\n");
  const tree = createProcessor({ remarkPlugins: [remarkGfm] }).parse(source) as unknown as MdNode;
  const ctx: Ctx = { source, depth: 1 };
  const text = convertRange(ctx, 0, source.length, tree.children ?? []);
  return tidy(text);
}

/**
 * Blank lines left by removed imports and comments, collapsed, and single
 * trailing spaces removed. Code blocks are left exactly as written.
 */
function tidy(text: string): string {
  const out: string[] = [];
  let fence: string | null = null;
  for (const line of text.split("\n")) {
    const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
    if (fence) {
      out.push(line);
      const closes = marker && marker[0] === fence[0] && marker.length >= fence.length && !line.trim().slice(marker.length);
      if (closes) fence = null;
      continue;
    }
    if (marker) fence = marker;
    const clean = line.replace(/[ \t]+$/, (spaces) => (spaces.length >= 2 ? spaces : ""));
    if (!clean.trim() && out.length && !out[out.length - 1].trim()) continue;
    out.push(clean.trim() ? clean : "");
  }
  return out.join("\n").trim();
}

/** The source between `from` and `to`, with every node in `nodes` converted. */
function convertRange(ctx: Ctx, from: number, to: number, nodes: MdNode[]): string {
  const edits: Edit[] = [];
  for (const node of nodes) collect(ctx, node, edits);
  edits.sort((a, b) => a.start - b.start);
  let out = "";
  let pos = from;
  for (const edit of edits) {
    if (edit.start < pos) continue;
    out += ctx.source.slice(pos, edit.start) + edit.text;
    pos = edit.end;
  }
  return out + ctx.source.slice(pos, to);
}

function collect(ctx: Ctx, node: MdNode, edits: Edit[]): void {
  switch (node.type) {
    case "heading":
      ctx.depth = node.depth ?? ctx.depth;
      for (const child of node.children ?? []) collect(ctx, child, edits);
      return;
    case "mdxjsEsm":
    case "mdxFlowExpression":
    case "mdxTextExpression":
      edits.push({ start: start(node), end: end(node), text: "" });
      return;
    case "mdxJsxFlowElement": {
      const indent = (node.position?.start.column ?? 1) - 1;
      let text = element(ctx, node, "flow");
      if (indent > 0) {
        text = text
          .split("\n")
          .map((line, i) => (i === 0 || !line ? line : " ".repeat(indent) + line))
          .join("\n");
      }
      edits.push({ start: start(node), end: end(node), text });
      return;
    }
    case "mdxJsxTextElement":
      edits.push({ start: start(node), end: end(node), text: element(ctx, node, "text") });
      return;
    case "code":
      edits.push({ start: start(node), end: end(node), text: cleanCode(ctx.source.slice(start(node), end(node))) });
      return;
    case "link":
    case "image":
    case "definition": {
      const url = node.url ?? "";
      const absolute = absoluteUrl(url);
      if (absolute !== url) {
        const kids = node.children ?? [];
        const after = kids.length ? end(kids[kids.length - 1]) : start(node) + 1;
        const at = ctx.source.indexOf(url, after);
        if (at !== -1 && at + url.length <= end(node)) edits.push({ start: at, end: at + url.length, text: absolute });
      }
      for (const child of node.children ?? []) collect(ctx, child, edits);
      return;
    }
    default:
      for (const child of node.children ?? []) collect(ctx, child, edits);
  }
}

/** Root-relative links become absolute; docs pages link their Markdown. */
/** Every `](/...)` link in generated Markdown, made absolute. */
function absoluteLinks(markdown: string): string {
  return markdown.replace(/\]\((\/[^)\s]*)\)/g, (_, url: string) => `](${absoluteUrl(url)})`);
}

export function absoluteUrl(url: string): string {
  if (!url.startsWith("/") || url.startsWith("//")) return url;
  const doc = /^\/docs\/([a-z0-9-]+)\/([a-z0-9-]+)\/?(#.*)?$/.exec(url);
  if (doc && docSlugs.has(`${doc[1]}/${doc[2]}`)) return `${site.url}${markdownPath(`${doc[1]}/${doc[2]}`)}${doc[3] ?? ""}`;
  return `${site.url}${url}`;
}

/**
 * A fenced code block without the site-only parts of its info string
 * (`{3-5}`, `wrap`) and without Shiki's notation comments. `[!code ++]`
 * and `[!code --]` become the words "added" and "removed".
 */
function cleanCode(block: string): string {
  const lines = block.split("\n");
  const open = /^(\s*)(`{3,}|~{3,})(.*)$/.exec(lines[0]);
  if (!open) return block;
  const [, indent, fence, info] = open;
  const lang = info.trim().split(/\s+/)[0] ?? "";
  const title = /(?:title|filename)="([^"]+)"/.exec(info)?.[1];
  lines[0] = `${indent}${fence}${lang}${title ? ` title="${title}"` : ""}`;
  for (let i = 1; i < lines.length; i++) {
    lines[i] = lines[i]
      .replace(/\s*(?:\/\/|#|--|;|\/\*|<!--)\s*\[!code (?:highlight|hl|focus|word)(?::[^\]]*)?\]\s*(?:\*\/|-->)?/g, "")
      .replace(/\[!code \+\+(?::\d+)?\]/g, "added")
      .replace(/\[!code --(?::\d+)?\]/g, "removed");
  }
  return lines.join("\n");
}

// ------------------------------------------------------------- components

/** The children of an element, converted, dedented and trimmed. */
function inner(ctx: Ctx, node: MdNode): string {
  const kids = node.children ?? [];
  if (!kids.length) return "";
  const column = (kids[0].position?.start.column ?? 1) - 1;
  const text = " ".repeat(column) + convertRange(ctx, start(kids[0]), end(kids[kids.length - 1]), kids);
  return dedent(text).trim();
}

function quote(text: string): string {
  return text
    .split("\n")
    .map((line) => (line ? `> ${line}` : ">"))
    .join("\n");
}

const isElement = (name: string) => (node: MdNode) =>
  (node.type === "mdxJsxFlowElement" || node.type === "mdxJsxTextElement") && node.name === name;

const calloutKinds: Record<string, string> = { note: "Note", tip: "Tip", warning: "Warning" };

/** What each architecture diagram shows, in one sentence. */
const diagrams: Record<string, string> = {
  "navree-ui":
    "How Navree UI works: a Rust handler returns components, which build one typed tree; render writes it to HTML in one pass with the page's content security policy; the browser loads navree-ui.css on every page and a module script only when the markup uses it; an action posts back and gets a Reply of ops that morph the page.",
  "modular-monolith":
    "A modular monolith: one Navree binary whose feature modules each own their routes, services and tables, talk through exported services and in-process events, and share one database.",
  microservices:
    "Microservices: an API gateway app forwards requests to separate Navree services, which call each other with request-response messages and publish events over a transport such as NATS, each with its own database.",
  "multi-tenancy":
    "Multi-tenancy with workspaces: each request is resolved to a workspace from a header, subdomain, path or token claim, and its data is isolated by a `workspace_id` row filter, a schema per workspace or a database per workspace.",
  "event-driven":
    "Event-driven: a module publishes a typed event on the event bus, and listeners in other modules react to it in process or on queue workers, without the publisher knowing who listens.",
  "api-first":
    "API-first: versioned, validated routes with access rules generate an OpenAPI 3.1 document, from which typed clients are generated for web, mobile and partner apps.",
  "backend-for-frontend":
    "Backend for frontend: the browser talks only to a front-end server that keeps the tokens in httpOnly cookies and calls the Navree API on the user's behalf, while real-time updates reach the browser over WebSockets.",
  "request-lifecycle":
    "The request lifecycle: a request passes through the HTTP server, global middleware, routing, guards and access rules, validation and the controller to a service, and the response or a JSON error returns the same way.",
  "module-graph":
    "The module graph: the root `AppModule` imports feature modules, each declaring the providers it registers, the modules it imports and the services it exports, and the container wires them at boot.",
  "reverse-engineering":
    "From an old product to a Navree app: reading the product (0 intake, 1 map), stop 1; schema and modules (2 base layer, 3 modules), stop 2; specs (4 module specs, 5 cross-cutting, 6 data migration, 7 reconcile), stop 3; plan (8 tasks and tests), stop 4; build and verify (9 build, 10 verify), stop 5; then cut-over. Phase n writes `specs/0n-*/`, and at each stop the owner confirms.",
  "design-to-ui":
    "From a design to Navree UI pages: reading the design (0 intake, 1 inventory), stop 1; mapping to Navree UI (2 tokens, 3 mapping, 4 reconcile), stop 2; build and verify (5 build, 6 verify), stop 3, where the owner accepts the pages seen beside the design. Phase n writes `design/0n-*/`.",
  "two-ways":
    "The two skills together: the old product's code and data go to navree-reverse-engineering, which builds the models, services and routes; its `screens.md`, `crud.md` and `lists.md` specs and the look (the old product's screenshots or a new design) go to navree-design-to-ui, which builds the Navree UI pages, one module at a time.",
};

/** A component as Markdown. `mode` is "flow" for blocks, "text" inside a paragraph. */
function element(ctx: Ctx, node: MdNode, mode: "flow" | "text"): string {
  const kids = node.children ?? [];
  switch (node.name) {
    case "Callout": {
      const kind = calloutKinds[attr(node, "type") ?? "note"] ?? "Note";
      const title = attr(node, "title");
      const head = `**${title ? `${kind}: ${title}` : kind}**`;
      const body = inner(ctx, node);
      return quote(body ? `${head}\n\n${body}` : head);
    }
    case "Tabs": {
      const base = ctx.depth;
      const hashes = "#".repeat(Math.min(base + 1, 6));
      const tabs = kids.filter(isElement("Tab"));
      const out = tabs.map((tab) => {
        ctx.depth = base + 1;
        const body = inner(ctx, tab);
        return `${hashes} ${attr(tab, "label") ?? "Tab"}${body ? `\n\n${body}` : ""}`;
      });
      ctx.depth = base;
      return out.join("\n\n");
    }
    case "CodeGroup": {
      const hashes = "#".repeat(Math.min(ctx.depth + 1, 6));
      return kids
        .filter((kid) => kid.type === "code")
        .map((code) => {
          const title = /(?:title|filename)="([^"]+)"/.exec(code.meta ?? "")?.[1] ?? code.lang ?? "Code";
          return `${hashes} ${title}\n\n${cleanCode(ctx.source.slice(start(code), end(code)))}`;
        })
        .join("\n\n");
    }
    case "Steps": {
      const base = ctx.depth;
      const hashes = "#".repeat(Math.min(base + 1, 6));
      const steps = kids.filter(isElement("Step"));
      const out = steps.map((step, i) => {
        ctx.depth = base + 1;
        const body = inner(ctx, step);
        return `${hashes} ${i + 1}. ${attr(step, "title") ?? `Step ${i + 1}`}${body ? `\n\n${body}` : ""}`;
      });
      ctx.depth = base;
      return out.join("\n\n");
    }
    case "LinkCards":
      return kids
        .filter(isElement("LinkCard"))
        .map((card) => linkCard(ctx, card))
        .join("\n");
    case "LinkCard":
      return linkCard(ctx, node);
    case "FileTree":
      return fileTree(ctx, node);
    case "Diagram": {
      const name = attr(node, "name") ?? "";
      const sentence = diagrams[name] ?? `A diagram of ${name.replace(/-/g, " ")}.`;
      return mode === "flow" ? `*Diagram: ${sentence}*` : sentence;
    }
    case "Badge": {
      const feature = attr(node, "feature");
      const text = feature ? `Feature: \`${feature}\`` : oneLine(inner(ctx, node));
      return text ? `(${text})` : "";
    }
    case "UiPreview":
      return previewMarkdown(attr(node, "example") ?? "");
    case "UiProps":
      return propsMarkdown(attr(node, "type") ?? "");
    case "UiAnatomy":
      return anatomyMarkdown(attr(node, "component") ?? "");
    case "UiChoose":
      return absoluteLinks(chooseMarkdown(attr(node, "name") ?? ""));
    case "UiGrid":
      return absoluteLinks(gridMarkdown());
    case "UiAssets":
      return assetsMarkdown();
    case "UiTokens":
      return tokensMarkdown();
    case "UiCsp":
      return cspMarkdown();
    case "br":
      return mode === "text" ? "\n" : "";
    default:
      return mode === "flow" ? inner(ctx, node) : oneLine(inner(ctx, node));
  }
}

function linkCard(ctx: Ctx, card: MdNode): string {
  const href = absoluteUrl(attr(card, "href") ?? "");
  const title = attr(card, "title") ?? href;
  const text = oneLine(inner(ctx, card));
  return `- [${title}](${href})${text ? `: ${text}` : ""}`;
}

/** `<FileTree>{`...`}</FileTree>` as a `text` code block, without the highlight stars. */
function fileTree(ctx: Ctx, node: MdNode): string {
  const expression = (node.children ?? []).find((kid) => kid.type === "mdxFlowExpression" || kid.type === "mdxTextExpression");
  // Read the tree from the source: the parsed expression has lost up to two
  // spaces of indentation on every line, and with them the first level.
  const written = expression ? fileTreeText(ctx.source, expression) : null;
  let raw = written ?? expression?.value?.trim() ?? inner(ctx, node);
  if (written === null) {
    if (raw.startsWith("`") && raw.endsWith("`")) raw = raw.slice(1, -1);
    raw = raw.replace(/\\`/g, "`").replace(/\\\$/g, "$").replace(/\\\\/g, "\\");
  }
  const lines = dedent(raw.replace(/^\s*\n|\n\s*$/g, ""))
    .split("\n")
    // `name *   # comment` highlights an entry on the page; keep the alignment.
    .map((line) => line.replace(/\s\*(?=\s|$)/, "  ").trimEnd());
  const title = attr(node, "title");
  return ["```text" + (title ? ` title="${title}"` : ""), ...lines, "```"].join("\n");
}
