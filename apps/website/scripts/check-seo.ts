/**
 * Checks the built site (`out/`) for what search engines and link previews
 * read. Run after `pnpm build`:
 *
 *   node scripts/check-seo.ts
 *
 * For every indexable page:
 * - a `<title>` of 30 to 60 characters and a meta description of 70 to 160,
 *   each unique across the site;
 * - one canonical URL that is the page's own address, absolute and on https;
 * - Open Graph tags (title, description, url, image with its size and alt,
 *   site name, type, locale) and the Twitter card tags, with the image alt;
 * - `<html lang="en">`, a viewport, and a robots meta that allows large
 *   previews;
 * - exactly one `<h1>`, and headings that never skip a level on the way down;
 * - alt text on every `<img>`;
 * - links to other pages, so no page is a dead end;
 * - JSON-LD that parses, uses only properties schema.org defines for its
 *   types, has the properties each type needs, resolves every `@id`
 *   reference, and carries the site graph, a page node and a breadcrumb
 *   whose last step is the page (`scripts/lib/jsonld.ts`);
 * - HTML under 650 KB, before compression.
 *
 * Pages marked noindex (the 404, the styleguide, redirects) are only
 * checked for the noindex. Also checks that robots.txt points at the
 * sitemap, that the sitemap lists every indexable page and nothing else,
 * each with a valid `lastmod` that is not in the future, and that the 404
 * page leads somewhere.
 *
 * `check-geo.ts` (AI crawlers and agent files), `check-aeo.ts` (questions
 * and steps) and `check-fresh.ts` (dates and the changelog) check the rest.
 */
import fs from "node:fs";
import path from "node:path";

import { checkPageJsonld, missingTypes } from "./lib/jsonld.ts";
import { OUT, SITE, finish, indexable, pages, requireOut } from "./lib/site-html.ts";
import { DESCRIPTION_MAX, DESCRIPTION_MIN, TITLE_MAX, TITLE_MIN } from "../src/lib/seo-text.ts";

requireOut("check-seo");

const MAX_HTML_KB = 650;
const problems: string[] = [];
const all = pages();
const titles = new Map<string, string>();
const descriptions = new Map<string, string>();

for (const page of all) {
  const { url } = page;
  const problem = (message: string) => problems.push(`${url}: ${message}`);

  if (url === "/404" || url === "/_not-found/") {
    if (!page.noindex) problem("the 404 page must be noindex");
    continue;
  }
  if (page.noindex) continue;

  // Title and description.
  const { title, description } = page;
  if (!title) problem("no <title>");
  else if (title.length < TITLE_MIN || title.length > TITLE_MAX) problem(`<title> is ${title.length} characters (${TITLE_MIN}-${TITLE_MAX}): "${title}"`);
  if (!description) problem("no meta description");
  else if (description.length < DESCRIPTION_MIN || description.length > DESCRIPTION_MAX) {
    problem(`meta description is ${description.length} characters (${DESCRIPTION_MIN}-${DESCRIPTION_MAX}): "${description.slice(0, 90)}..."`);
  }
  if (titles.has(title)) problem(`title "${title}" is also the title of ${titles.get(title)}`);
  if (descriptions.has(description)) problem(`description is also the description of ${descriptions.get(description)}`);
  titles.set(title, url);
  descriptions.set(description, url);

  // Canonical.
  const canonicals = page.head.match(/<link rel="canonical"/g)?.length ?? 0;
  if (canonicals !== 1) problem(`${canonicals} canonical links`);
  if (page.canonical !== `${SITE}${url}`) problem(`canonical is ${page.canonical || "missing"}, expected ${SITE}${url}`);

  // Social tags.
  for (const key of ["og:title", "og:description", "og:image", "og:image:alt", "og:image:width", "og:image:height", "og:site_name", "og:type", "og:locale"]) {
    if (!page.meta(key)) problem(`no ${key}`);
  }
  if (page.meta("og:url") !== `${SITE}${url}`) problem(`og:url is ${page.meta("og:url") || "missing"}`);
  if (page.meta("og:image") && !page.meta("og:image").startsWith("https://")) problem(`og:image is not an absolute https URL: ${page.meta("og:image")}`);
  for (const key of ["twitter:card", "twitter:title", "twitter:description", "twitter:image", "twitter:image:alt"]) {
    if (!page.meta(key)) problem(`no ${key}`);
  }
  if (page.meta("twitter:card") && page.meta("twitter:card") !== "summary_large_image") problem(`twitter:card is ${page.meta("twitter:card")}`);

  // Document basics.
  if (page.lang !== "en") problem(`<html lang> is "${page.lang}", expected "en"`);
  if (!/<meta name="viewport"/.test(page.head)) problem("no viewport meta");
  const robots = page.meta("robots");
  if (robots && !/max-image-preview:large/.test(robots)) problem(`robots meta "${robots}" does not allow large previews`);

  // Headings: one h1, never a skipped level on the way down.
  const headings = [...page.body.matchAll(/<h([1-6])\b[^>]*>([^]*?)<\/h\1>/g)].map((m) => ({ level: Number(m[1]), text: m[2].replace(/<[^>]+>/g, "").trim() }));
  const h1s = headings.filter((h) => h.level === 1);
  if (h1s.length !== 1) problem(`${h1s.length} <h1> elements`);
  else if (!h1s[0].text) problem("the <h1> is empty");
  for (let i = 1; i < headings.length; i++) {
    if (headings[i].level - headings[i - 1].level > 1) {
      problem(`heading order jumps from h${headings[i - 1].level} to h${headings[i].level} ("${headings[i].text.slice(0, 40)}")`);
      break;
    }
  }

  // Images and links.
  for (const img of page.body.match(/<img\b[^>]*>/g) ?? []) {
    if (!/\balt="/.test(img)) problem(`<img> without alt: ${img.slice(0, 120)}`);
  }
  const links = (page.body.match(/<a\b[^>]*\bhref="\/[^"/][^"]*"/g) ?? []).length;
  if (links < 3) problem(`only ${links} internal links; a page needs a way on`);

  // Structured data.
  for (const message of checkPageJsonld(page)) problem(message);
  const missing = missingTypes(page);
  if (missing.length) problem(`JSON-LD lacks ${missing.join(", ")}`);

  // Weight.
  const kb = Math.round(Buffer.byteLength(page.html) / 1024);
  if (kb > MAX_HTML_KB) problem(`the HTML is ${kb} KB, over ${MAX_HTML_KB} KB`);
}

// The 404 page leads somewhere.
const notFound = all.find((page) => page.url === "/404");
if (!notFound) problems.push("404.html is missing");
else if (!/<a\b[^>]*\bhref="\/"/.test(notFound.body)) problems.push("404.html does not link to the homepage");

// robots.txt.
const robots = fs.readFileSync(path.join(OUT, "robots.txt"), "utf8");
if (!/User-Agent: \*/i.test(robots) || !/Allow: \//.test(robots) || !robots.includes(`Sitemap: ${SITE}/sitemap.xml`)) {
  problems.push(`robots.txt: expected User-Agent: *, Allow: / and the sitemap line, got:\n${robots}`);
}
if (/^Disallow:\s*\S/im.test(robots)) problems.push("robots.txt disallows a path; the site is public");

// sitemap.xml.
const indexableUrls = new Set(indexable(all).map((page) => page.url));
const sitemap = fs.readFileSync(path.join(OUT, "sitemap.xml"), "utf8");
const entries = [...sitemap.matchAll(/<url>([^]*?)<\/url>/g)].map((m) => ({
  loc: /<loc>([^<]+)<\/loc>/.exec(m[1])?.[1] ?? "",
  lastmod: /<lastmod>([^<]+)<\/lastmod>/.exec(m[1])?.[1],
}));
const listed = new Set(entries.map((entry) => entry.loc.replace(SITE, "")));
if (listed.size !== entries.length) problems.push("sitemap.xml: a URL is listed twice");
for (const url of indexableUrls) if (!listed.has(url)) problems.push(`sitemap.xml: ${url} is missing`);
for (const url of listed) if (!indexableUrls.has(url)) problems.push(`sitemap.xml: ${url} is listed but isn't an indexable page`);
for (const entry of entries) {
  if (!entry.loc.startsWith(`${SITE}/`)) problems.push(`sitemap.xml: ${entry.loc} is not an absolute ${SITE} URL`);
  if (!entry.lastmod) problems.push(`sitemap.xml: ${entry.loc} has no <lastmod>`);
  else if (Number.isNaN(Date.parse(entry.lastmod))) problems.push(`sitemap.xml: ${entry.loc} has lastmod "${entry.lastmod}", not a date`);
  else if (Date.parse(entry.lastmod) > Date.now() + 24 * 3600 * 1000) problems.push(`sitemap.xml: ${entry.loc} has a lastmod in the future (${entry.lastmod})`);
}

finish(
  "check-seo",
  problems,
  `${indexableUrls.size} indexable pages, all in the sitemap; titles (${TITLE_MIN}-${TITLE_MAX}), descriptions (${DESCRIPTION_MIN}-${DESCRIPTION_MAX}), canonicals, social tags, one h1, alt text and JSON-LD are in order`,
);
