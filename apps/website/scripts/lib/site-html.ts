/**
 * What the check scripts share: reading the static export in `out/` page by
 * page and pulling out what search engines, link previews and AI crawlers
 * read. Plain regular expressions over Next's HTML, which is regular enough
 * for this; no dependencies. Only erasable TypeScript: Node runs it as is.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const WEBSITE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
export const OUT = path.join(WEBSITE, "out");
export const SITE = "https://navree.com";

export function requireOut(script: string): void {
  if (!fs.existsSync(OUT)) {
    console.error(`${script}: out/ does not exist; run \`pnpm build\` first`);
    process.exit(1);
  }
}

/** Every `.html` file under `dir`, leaving out Next's `_next` assets. */
export function htmlFiles(dir: string = OUT): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "_next" ? [] : htmlFiles(full);
    return entry.name.endsWith(".html") ? [full] : [];
  });
}

export const decode = (s: string) =>
  s
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&#39;/g, "'");

export type Jsonld = Record<string, unknown>;

export type Page = {
  /** `/docs/basics/validation/`, or `/404`. */
  url: string;
  file: string;
  html: string;
  head: string;
  body: string;
  noindex: boolean;
  lang: string;
  title: string;
  description: string;
  canonical: string;
  /** `<meta name=... content=...>` and `<meta property=... content=...>`, by key. */
  meta: (key: string) => string;
  /** Every JSON-LD script: the parsed value, or the parse error. */
  jsonld: { data?: Jsonld; error?: string; raw: string }[];
  /** The visible text of the body, scripts and styles removed. */
  text: string;
};

/** The `index.html` and `404.html` of the export, read and split. */
export function pages(): Page[] {
  const out: Page[] = [];
  for (const file of htmlFiles()) {
    const rel = path.relative(OUT, file).split(path.sep).join("/");
    if (rel !== "404.html" && !rel.endsWith("index.html")) continue;
    const url = rel === "404.html" ? "/404" : `/${rel.replace(/index\.html$/, "")}`;
    const html = fs.readFileSync(file, "utf8");
    const headEnd = html.indexOf("</head>");
    const head = html.slice(0, headEnd);
    const body = html.slice(html.indexOf("<body"));
    const metaOf = (key: string) =>
      decode(
        new RegExp(`<meta (?:name|property)="${key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}" content="([^"]*)"`).exec(head)?.[1] ?? "",
      );
    const jsonld = [...html.matchAll(/<script type="application\/ld\+json">([^]*?)<\/script>/g)].map((m) => {
      try {
        return { data: JSON.parse(m[1]) as Jsonld, raw: m[1] };
      } catch (error) {
        return { error: String(error), raw: m[1] };
      }
    });
    out.push({
      url,
      file,
      html,
      head,
      body,
      noindex: /<meta name="robots" content="[^"]*noindex/.test(head),
      lang: /<html[^>]*\blang="([^"]*)"/.exec(html)?.[1] ?? "",
      title: decode(/<title>([^<]*)<\/title>/.exec(head)?.[1] ?? ""),
      description: metaOf("description"),
      canonical: /<link rel="canonical" href="([^"]*)"/.exec(head)?.[1] ?? "",
      meta: metaOf,
      jsonld,
      text: decode(
        body
          .replace(/<script[^]*?<\/script>/g, " ")
          .replace(/<style[^]*?<\/style>/g, " ")
          .replace(/<[^>]+>/g, " "),
      )
        .replace(/\s+/g, " ")
        .trim(),
    });
  }
  return out;
}

/** The nodes of a page's JSON-LD: every `@graph` member and every top-level object. */
export function nodes(page: Page): Jsonld[] {
  return page.jsonld.flatMap((block) => {
    if (!block.data) return [];
    const graph = block.data["@graph"];
    return Array.isArray(graph) ? (graph as Jsonld[]) : [block.data];
  });
}

export const typesOf = (node: Jsonld): string[] => {
  const t = node["@type"];
  return Array.isArray(t) ? (t as string[]) : t ? [String(t)] : [];
};

/** Indexable pages: not the 404, not noindex. */
export const indexable = (all: Page[]) => all.filter((page) => !page.noindex && page.url !== "/404" && page.url !== "/_not-found/");

/** Reports problems the way every check does and sets the exit code. */
export function finish(script: string, problems: string[], ok: string): void {
  if (problems.length) {
    console.error(problems.join("\n"));
    console.error(`\n${script}: ${problems.length} problem${problems.length === 1 ? "" : "s"}`);
    process.exit(1);
  }
  console.log(`${script}: ${ok}`);
}
