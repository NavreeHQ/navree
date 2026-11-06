/**
 * Checks the 404 page: run after `pnpm build`.
 *
 *   node scripts/check-404.ts
 *
 * - `404.html` has a search field in the page (a form with role="search" and
 *   a search input), not only a hint about a keyboard shortcut, and keeps its
 *   links to the docs and the homepage;
 * - `data/pages.json`, the list the page reads to suggest the nearest page,
 *   names a page of the export for every entry, and holds every docs page;
 * - the nearest page to a mistyped address is the one meant: a docs page
 *   asked for without its section, a misspelt slug, an address the site used
 *   to have, and no suggestion for an address that resembles nothing.
 */
import fs from "node:fs";
import path from "node:path";

import { sections } from "../content/docs/manifest.ts";
import { closestPage, type PageEntry } from "../src/lib/closest-page.ts";
import { OUT, finish, pages, requireOut } from "./lib/site-html.ts";

requireOut("check-404");
const problems: string[] = [];

// ------------------------------------------------------------ the page itself

const notFound = pages().find((page) => page.url === "/404");
if (!notFound) problems.push("404.html is missing");
else {
  if (!/<form[^>]*\brole="search"/.test(notFound.body)) problems.push("404.html has no search form in the page");
  if (!/<input[^>]*\btype="search"/.test(notFound.body)) problems.push("404.html has no search field in the page");
  if (!/<a\b[^>]*\bhref="\/docs\/"/.test(notFound.body)) problems.push("404.html does not link to the docs");
  if (!/<a\b[^>]*\bhref="\/"/.test(notFound.body)) problems.push("404.html does not link to the homepage");
  if ((notFound.body.match(/<h1\b/g) ?? []).length !== 1) problems.push("404.html must have exactly one h1");
}

// ------------------------------------------------------- the list of pages

const listFile = path.join(OUT, "data", "pages.json");
let list: PageEntry[] = [];
if (!fs.existsSync(listFile)) problems.push("data/pages.json is missing");
else {
  list = JSON.parse(fs.readFileSync(listFile, "utf8")) as PageEntry[];
  const size = fs.statSync(listFile).size;
  if (size > 100 * 1024) problems.push(`data/pages.json is ${Math.round(size / 1024)} KB; the 404 page loads it for a suggestion, keep it small`);
  for (const entry of list) {
    if (!entry.href.startsWith("/") || !entry.title) problems.push(`data/pages.json has a malformed entry: ${JSON.stringify(entry)}`);
    else if (!fs.existsSync(path.join(OUT, entry.href, "index.html"))) problems.push(`data/pages.json lists ${entry.href}, which the export does not have`);
  }
  const have = new Set(list.map((entry) => entry.href));
  for (const section of sections) {
    for (const page of section.pages) {
      if (!have.has(`/docs/${section.slug}/${page}/`)) problems.push(`data/pages.json does not list /docs/${section.slug}/${page}/`);
    }
  }
}

// ------------------------------------------------------ the nearest page

if (list.length) {
  const expect = (asked: string, wanted: string | null) => {
    const got = closestPage(asked, list)?.href ?? null;
    if (got !== wanted) problems.push(`the nearest page to ${asked} is ${got ?? "none"}, expected ${wanted ?? "none"}`);
  };
  expect("/docs/validation/", "/docs/basics/validation/");
  expect("/docs/basics/validaton/", "/docs/basics/validation/");
  expect("/docs/basics/validation", "/docs/basics/validation/");
  expect("/docs/Basics/Routing/", "/docs/basics/routing/");
  expect("/benchmarks/", "/performance/");
  expect("/perfomance/", "/performance/");
  expect("/zzzz-qqqq/", null);
}

finish("check-404", problems, `the 404 page has a search field and suggests the nearest of ${list.length} pages`);
