/**
 * Checks every internal link in the static export, after `pnpm build`:
 *
 *   node scripts/check-links.ts          (checks out/)
 *
 * Each `href`, `src` and `srcset` in every HTML page (but the UI previews'
 * example links) must resolve to a file
 * in `out/`, and each `#anchor` to an id on the target page. The navree.com
 * links inside `llms.txt`, `llms-full.txt` and the Markdown copies of the
 * docs are checked the same way (a `.md` link's anchor against its page).
 * Exits with 1 and lists each broken link with the pages it is on.
 *
 * The launch film's files (`/media/`) are not in git, so a link to one is
 * checked against `content/media.json`, which lists every file with its size
 * and SHA-256: the name must be listed, and every listed file must be in
 * `out/media/` with exactly those bytes (build with NAVREE_MEDIA_DIR set, or
 * NAVREE_MEDIA_URL, so `scripts/prebuild.ts` copies them in).
 */
import fs from "node:fs";
import path from "node:path";

import { MEDIA_URL, problemWith, readManifest } from "./lib/media.ts";

const OUT = path.resolve(import.meta.dirname, "..", "out");
const SITE = "https://navree.com";

if (!fs.existsSync(OUT)) {
  console.error("check-links: out/ does not exist; run `pnpm build` first");
  process.exit(1);
}

function walk(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    return entry.isDirectory() ? walk(full) : [full];
  });
}

const rel = (file: string) => path.relative(OUT, file).split(path.sep).join("/");
const decode = (s: string) => s.replace(/&amp;/g, "&").replace(/&#x27;/g, "'").replace(/&quot;/g, '"');

/** The file a site path serves, or null. */
function resolveFile(urlPath: string): string | null {
  let p = decodeURIComponent(urlPath);
  if (p.endsWith("/")) p += "index.html";
  const file = path.join(OUT, p);
  if (fs.existsSync(file) && fs.statSync(file).isFile()) return file;
  if (fs.existsSync(`${file}.html`)) return `${file}.html`;
  const index = path.join(file, "index.html");
  return fs.existsSync(index) ? index : null;
}

const media = readManifest();
const mediaNames = new Set(media.map((file) => file.name));

const idCache = new Map<string, Set<string>>();
function idsOf(file: string): Set<string> {
  let ids = idCache.get(file);
  if (!ids) {
    const html = fs.readFileSync(file, "utf8");
    ids = new Set([...html.matchAll(/\s(?:id|name)="([^"]+)"/g)].map((m) => decode(m[1])));
    idCache.set(file, ids);
  }
  return ids;
}

const problems = new Map<string, Set<string>>();
let checked = 0;
let anchors = 0;

function report(from: string, link: string, why: string) {
  const key = `${link}  (${why})`;
  if (!problems.has(key)) problems.set(key, new Set());
  problems.get(key)!.add(rel(from));
}

function check(from: string, raw: string) {
  let link = decode(raw.trim());
  if (!link || /^(mailto|tel|javascript|data):/.test(link)) return;
  if (link.startsWith(SITE)) link = link.slice(SITE.length) || "/";
  if (/^[a-z]+:/i.test(link) || link.startsWith("//")) return; // another site
  checked++;
  const [beforeHash, hash] = link.split("#");
  const urlPath = beforeHash.split("?")[0];
  if (urlPath.startsWith(MEDIA_URL)) {
    // The files themselves are checked once, against the manifest, below.
    if (!mediaNames.has(decodeURIComponent(urlPath.slice(MEDIA_URL.length)))) report(from, link, "not in content/media.json");
    return;
  }
  let target: string | null = from;
  if (urlPath !== "") {
    const absolute = urlPath.startsWith("/") ? urlPath : path.posix.join(`/${rel(path.dirname(from))}`, urlPath);
    target = resolveFile(absolute);
    if (!target) return report(from, link, "no such file");
  }
  // A Markdown copy's anchors are its HTML page's.
  if (hash && target.endsWith(".md")) target = resolveFile(`/${rel(target).replace(/\.md$/, "/")}`) ?? target;
  if (hash && target.endsWith(".html")) {
    anchors++;
    if (!idsOf(target).has(decodeURIComponent(hash))) report(from, link, "no such anchor");
  }
}

const files = walk(OUT);
// The UI previews (`ui-preview/`) are pages of an example app: their links
// (`/deals/42`) show what a component draws and go nowhere by design. Their
// own files (the toolkit's stylesheet, font and modules) are still checked.
const html = files.filter((f) => f.endsWith(".html") && !rel(f).startsWith("ui-preview/"));
const previews = files.filter((f) => f.endsWith(".html") && rel(f).startsWith("ui-preview/"));
for (const file of previews) {
  const text = fs.readFileSync(file, "utf8");
  for (const m of text.matchAll(/<(?:link|script)\s[^>]*(?:href|src)="([^"]+)"/g)) {
    checked++;
    if (!fs.existsSync(path.resolve(path.dirname(file), m[1]))) report(rel(file), m[1], "no such file");
  }
}
for (const file of html) {
  const text = fs.readFileSync(file, "utf8");
  // Attribute values only: the router's JSON payload is not markup.
  for (const m of text.matchAll(/\s(?:href|src)="([^"]*)"/g)) check(file, m[1]);
  for (const m of text.matchAll(/\ssrcset="([^"]*)"/g)) for (const part of m[1].split(",")) check(file, part.trim().split(/\s+/)[0]);
}

// The export holds exactly the files the manifest lists, byte for byte.
for (const file of media) {
  const problem = problemWith(file, path.join(OUT, "media"));
  if (problem) report(path.join(OUT, "media"), `${MEDIA_URL}${file.name}`, problem.endsWith(" is missing") ? `${problem}; build with NAVREE_MEDIA_DIR set` : problem);
}

const markdown = files.filter((f) => f.endsWith(".md") || f.endsWith("llms.txt") || f.endsWith("llms-full.txt"));
for (const file of markdown) {
  const text = fs.readFileSync(file, "utf8");
  for (const m of text.matchAll(/(?<![\w])(https:\/\/navree\.com\/[^\s)`"'<>\]]*)(.?)/g)) {
    if (m[2] === "<" || m[2] === "{") continue; // a placeholder such as /skills/<name>/
    check(file, m[1].replace(/[.,;:]+$/, ""));
  }
}

console.log(`check-links: ${html.length} HTML pages, ${previews.length} UI previews and ${markdown.length} Markdown files, ${checked} internal links (${anchors} with an anchor), ${media.length} /media/ files against content/media.json`);
if (problems.size === 0) {
  console.log("OK: every internal link resolves");
} else {
  console.log(`${problems.size} broken link(s):`);
  for (const [problem, from] of problems) {
    const pages = [...from];
    console.log(`  ${problem}\n    on ${pages.slice(0, 5).join(", ")}${pages.length > 5 ? ` and ${pages.length - 5} more` : ""}`);
  }
  process.exitCode = 1;
}
