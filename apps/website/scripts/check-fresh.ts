/**
 * Checks that the site shows what changed: run after `pnpm build`.
 *
 *   node scripts/check-fresh.ts
 *
 * - `/changelog/` exists, lists changes newest first and day by day, links
 *   each one to its commit, and has no date in the future;
 * - the Atom feed parses as a feed and its newest entry is the page's newest day;
 * - the sitemap's `lastmod` of `/changelog/` is the date of the newest commit;
 * - every docs page's `dateModified` is the `lastmod` the sitemap gives it,
 *   and so is every other page's: one date per page, from git;
 * - the IndexNow key file is in the export and `deploy/navree.com/indexnow.sh`
 *   uses the same key.
 */
import fs from "node:fs";
import path from "node:path";

import { OUT, SITE, WEBSITE, finish, nodes, pages, requireOut, typesOf } from "./lib/site-html.ts";
import { changelog } from "../src/lib/changelog.ts";
import { latestCommit } from "../src/lib/last-modified.ts";

requireOut("check-fresh");
process.chdir(WEBSITE);

const problems: string[] = [];
const all = pages();
const today = new Date(Date.now() + 24 * 3600 * 1000).toISOString().slice(0, 10);

// ----------------------------------------------------------- the changelog

const page = all.find((p) => p.url === "/changelog/");
const days = changelog();
if (!page) problems.push("/changelog/ is missing");
else {
  const ids = [...page.body.matchAll(/<section[^>]*\sid="(\d{4}-\d{2}-\d{2})"/g)].map((m) => m[1]);
  if (!ids.length) problems.push("/changelog/ lists no days");
  else {
    if (ids.some((id, i) => i > 0 && id >= ids[i - 1])) problems.push("/changelog/: days are not newest first");
    if (ids[0] > today) problems.push(`/changelog/: the newest day (${ids[0]}) is in the future`);
    if (days.length && ids[0] !== days[0].day) problems.push(`/changelog/: the newest day is ${ids[0]}, the commit history says ${days[0].day}`);
    const listed = (page.body.match(/href="https:\/\/github\.com\/NavreeHQ\/navree\/commit\/[0-9a-f]{7,40}"/g) ?? []).length;
    const expected = days.reduce((n, day) => n + day.changes.length, 0);
    if (listed < expected) problems.push(`/changelog/ links ${listed} commits, the history has ${expected} feat and fix commits`);
  }
}

// ---------------------------------------------------------------- the feed

const feedFile = path.join(OUT, "changelog", "feed.xml");
if (!fs.existsSync(feedFile)) problems.push("/changelog/feed.xml is missing");
else {
  const feed = fs.readFileSync(feedFile, "utf8");
  if (!/^<\?xml[^>]*\?>\s*<feed xmlns="http:\/\/www\.w3\.org\/2005\/Atom">/.test(feed)) problems.push("/changelog/feed.xml is not an Atom feed");
  const entries = [...feed.matchAll(/<entry>([^]*?)<\/entry>/g)].map((m) => m[1]);
  if (!entries.length && days.length) problems.push("/changelog/feed.xml has no entries");
  for (const entry of entries) {
    for (const tag of ["id", "title", "updated", "content"]) if (!new RegExp(`<${tag}[ >]`).test(entry)) problems.push(`/changelog/feed.xml: an entry has no <${tag}>`);
  }
  const first = /<id>[^<]*#(\d{4}-\d{2}-\d{2})<\/id>/.exec(entries[0] ?? "")?.[1];
  if (first && days.length && first !== days[0].day) problems.push(`/changelog/feed.xml: the newest entry is ${first}, the history says ${days[0].day}`);
  if (!all.some((p) => p.head.includes(`href="/changelog/feed.xml"`) || p.head.includes(`href="${SITE}/changelog/feed.xml"`))) {
    problems.push("no page links the feed in its <head>");
  }
}

// ------------------------------------------------------- one date per page

const sitemap = fs.readFileSync(path.join(OUT, "sitemap.xml"), "utf8");
const lastmod = new Map<string, string>();
for (const m of sitemap.matchAll(/<url>\s*<loc>([^<]+)<\/loc>\s*<lastmod>([^<]+)<\/lastmod>/g)) lastmod.set(m[1].replace(SITE, ""), m[2]);

const newest = latestCommit();
const changelogMod = lastmod.get("/changelog/");
if (changelogMod && Date.parse(changelogMod) !== Date.parse(newest)) {
  problems.push(`sitemap.xml: /changelog/ has lastmod ${changelogMod}, the newest commit is ${newest}`);
}

let dated = 0;
for (const p of all) {
  const expected = lastmod.get(p.url);
  if (!expected) continue;
  const modified = nodes(p)
    .filter((n) => typesOf(n).some((t) => t === "WebPage" || t === "CollectionPage" || t === "TechArticle"))
    .map((n) => n.dateModified)
    .filter(Boolean);
  if (!modified.length) {
    problems.push(`${p.url}: no dateModified in the structured data`);
    continue;
  }
  dated++;
  for (const when of modified) {
    if (Date.parse(String(when)) !== Date.parse(expected)) problems.push(`${p.url}: dateModified ${String(when)} differs from the sitemap's lastmod ${expected}`);
  }
}

// ---------------------------------------------------------------- IndexNow

const script = path.join(WEBSITE, "..", "..", "deploy", "navree.com", "indexnow.sh");
const keyOfScript = fs.existsSync(script) ? /^INDEXNOW_KEY="?([A-Za-z0-9-]{8,128})"?$/m.exec(fs.readFileSync(script, "utf8"))?.[1] : undefined;
if (!keyOfScript) problems.push("deploy/navree.com/indexnow.sh has no INDEXNOW_KEY line");
else {
  const keyFile = path.join(OUT, `${keyOfScript}.txt`);
  if (!fs.existsSync(keyFile)) problems.push(`the IndexNow key file ${keyOfScript}.txt is not in the export (put it in apps/website/public/)`);
  else if (fs.readFileSync(keyFile, "utf8").trim() !== keyOfScript) problems.push(`${keyOfScript}.txt must contain exactly the key`);
}

finish(
  "check-fresh",
  problems,
  `changelog with ${days.length} days (newest ${days[0]?.day ?? "none"}), its feed, ${dated} pages with one git date in the sitemap and the structured data, the IndexNow key in place`,
);
