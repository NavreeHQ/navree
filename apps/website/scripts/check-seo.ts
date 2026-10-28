/**
 * Checks the built site (`out/`) for what search engines and link previews
 * read. Run after `pnpm build`:
 *
 *   node scripts/check-seo.ts
 *
 * For every indexable page: exactly one <h1>; a title and a description, each
 * unique across the site; a canonical URL that matches the page; og:title,
 * og:description, og:url, og:image and the Twitter card tags; alt text on
 * every <img>. Pages marked noindex (the 404, the styleguide, redirects) are
 * only checked for the noindex. Also checks that robots.txt points at the
 * sitemap and that the sitemap lists every indexable page and nothing else.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OUT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "out");
const SITE = "https://navree.com";

function files(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "_next" ? [] : files(full);
    return entry.name.endsWith(".html") ? [full] : [];
  });
}

const decode = (s: string) =>
  s.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&#39;/g, "'");
const meta = (html: string, attr: "name" | "property", key: string) =>
  decode(new RegExp(`<meta ${attr}="${key}" content="([^"]*)"`).exec(html)?.[1] ?? "");

const problems: string[] = [];
const titles = new Map<string, string>();
const descriptions = new Map<string, string>();
const indexable = new Set<string>();

for (const file of files(OUT)) {
  const rel = path.relative(OUT, file).split(path.sep).join("/");
  const url = rel === "404.html" ? "/404" : `/${rel.replace(/index\.html$/, "")}`;
  if (rel !== "404.html" && !rel.endsWith("index.html")) continue;
  const html = fs.readFileSync(file, "utf8");
  const head = html.slice(0, html.indexOf("</head>"));
  const noindex = /<meta name="robots" content="[^"]*noindex/.test(head);
  const problem = (message: string) => problems.push(`${url}: ${message}`);

  if (url === "/404" || url === "/404/" || url === "/_not-found/") {
    if (!noindex) problem("the 404 page must be noindex");
    continue;
  }
  if (noindex) continue;
  indexable.add(url);

  // The page's own <title>, not the <title>s inside inline SVG diagrams.
  const title = decode(/<title>([^<]*)<\/title>/.exec(head)?.[1] ?? "");
  const description = meta(head, "name", "description");
  if (!title) problem("no <title>");
  if (!description) problem("no meta description");
  if (titles.has(title)) problem(`title "${title}" is also the title of ${titles.get(title)}`);
  if (descriptions.has(description)) problem(`description is also the description of ${descriptions.get(description)}`);
  titles.set(title, url);
  descriptions.set(description, url);

  const canonical = /<link rel="canonical" href="([^"]*)"/.exec(head)?.[1];
  if (canonical !== `${SITE}${url}`) problem(`canonical is ${canonical ?? "missing"}, expected ${SITE}${url}`);
  for (const key of ["og:title", "og:description", "og:image", "og:site_name", "og:type"]) {
    if (!meta(head, "property", key)) problem(`no ${key}`);
  }
  if (meta(head, "property", "og:url") !== `${SITE}${url}`) problem(`og:url is ${meta(head, "property", "og:url") || "missing"}`);
  for (const key of ["twitter:card", "twitter:title", "twitter:description", "twitter:image"]) {
    if (!meta(head, "name", key)) problem(`no ${key}`);
  }

  const body = html.slice(html.indexOf("<body"));
  const h1s = body.match(/<h1[\s>]/g)?.length ?? 0;
  if (h1s !== 1) problem(`${h1s} <h1> elements`);
  for (const img of body.match(/<img\b[^>]*>/g) ?? []) {
    if (!/\balt="/.test(img)) problem(`<img> without alt: ${img.slice(0, 120)}`);
  }
}

const robots = fs.readFileSync(path.join(OUT, "robots.txt"), "utf8");
if (!/User-Agent: \*/i.test(robots) || !/Allow: \//.test(robots) || !robots.includes(`Sitemap: ${SITE}/sitemap.xml`)) {
  problems.push(`robots.txt: expected User-Agent: *, Allow: / and the sitemap line, got:\n${robots}`);
}
const sitemap = fs.readFileSync(path.join(OUT, "sitemap.xml"), "utf8");
const listed = new Set([...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => m[1].replace(SITE, "")));
for (const url of indexable) if (!listed.has(url)) problems.push(`sitemap.xml: ${url} is missing`);
for (const url of listed) if (!indexable.has(url)) problems.push(`sitemap.xml: ${url} is listed but isn't an indexable page`);
if ((sitemap.match(/<lastmod>/g)?.length ?? 0) !== listed.size) problems.push("sitemap.xml: every URL needs a <lastmod>");

if (problems.length) {
  console.error(problems.join("\n"));
  console.error(`\ncheck-seo: ${problems.length} problems`);
  process.exit(1);
}
console.log(`check-seo: ${indexable.size} indexable pages, all in the sitemap; titles, descriptions, canonicals, social tags, one h1 and alt text are in order`);
