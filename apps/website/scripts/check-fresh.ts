/**
 * Checks that the site shows what changed: run after `pnpm build`.
 *
 *   node scripts/check-fresh.ts
 *
 * - `/changelog/` exists, lists weeks newest first, opens each with a
 *   highlight of two lines at most, keeps every area closed, links each
 *   release to its commit, shows none of the internal scopes, and has no
 *   week in the future;
 * - the Atom feed parses as a feed and its newest entry is the page's newest week;
 * - every scope the changelog leaves out as retired is really gone from the
 *   repository, and neither the page nor the feed shows it;
 * - the sitemap's `lastmod` of `/changelog/` is the date of the newest commit;
 * - every docs page's `dateModified` is the `lastmod` the sitemap gives it,
 *   and so is every other page's: one date per page, from git;
 * - the IndexNow key file is in the export and `deploy/navree.com/indexnow.sh`
 *   uses the same key.
 */
import fs from "node:fs";
import path from "node:path";

import { OUT, SITE, WEBSITE, finish, nodes, pages, requireOut, typesOf } from "./lib/site-html.ts";
import { HIGHLIGHT_MAX, INTERNAL_SCOPES, RETIRED_SCOPES, changelog, weekStart } from "../src/lib/changelog.ts";
import { latestCommit } from "../src/lib/last-modified.ts";

requireOut("check-fresh");
process.chdir(WEBSITE);

const problems: string[] = [];
const all = pages();
const today = new Date(Date.now() + 24 * 3600 * 1000).toISOString().slice(0, 10);

// ----------------------------------------------------------- the changelog

const page = all.find((p) => p.url === "/changelog/");
const weeks = changelog();
if (!weeks.length) problems.push("changelog.ts has no weeks");
for (const week of weeks) {
  if (weekStart(week.start) !== week.start) problems.push(`changelog.ts: the week ${week.start} does not start on a Monday`);
  if (week.highlight.length > HIGHLIGHT_MAX) problems.push(`changelog.ts: the highlight of ${week.start} is ${week.highlight.length} characters, the limit is ${HIGHLIGHT_MAX}`);
  for (const release of week.releases) {
    if (weekStart(release.day) !== week.start) problems.push(`changelog.ts: ${release.version} (${release.day}) is not in the week of ${week.start}`);
  }
  for (const area of week.areas) {
    for (const change of area.changes) {
      if (INTERNAL_SCOPES.includes(change.scope)) problems.push(`changelog.ts: the change "${change.subject}" is in the internal scope "${change.scope}"`);
    }
  }
}
if (!page) problems.push("/changelog/ is missing");
else {
  const ids = [...page.body.matchAll(/<section[^>]*\sid="week-(\d{4}-\d{2}-\d{2})"/g)].map((m) => m[1]);
  if (!ids.length) problems.push("/changelog/ lists no weeks");
  else {
    if (ids.some((id, i) => i > 0 && id >= ids[i - 1])) problems.push("/changelog/: weeks are not newest first");
    if (ids[0] > today) problems.push(`/changelog/: the newest week (${ids[0]}) is in the future`);
    if (weeks.length && ids[0] !== weeks[0].start) problems.push(`/changelog/: the newest week is ${ids[0]}, changelog.ts says ${weeks[0].start}`);
    const shown = (page.body.match(/<li[^>]*\sdata-change=""/g) ?? []).length;
    const expected = weeks.reduce((n, week) => n + week.count, 0);
    if (shown !== expected) problems.push(`/changelog/ lists ${shown} changes, changelog.ts has ${expected}`);
    const commits = (page.body.match(/href="https:\/\/github\.com\/NavreeHQ\/navree\/commit\/[0-9a-f]{7,40}"/g) ?? []).length;
    const releases = weeks.reduce((n, week) => n + week.releases.length, 0);
    if (commits < releases) problems.push(`/changelog/ links ${commits} release commits, there are ${releases} releases`);
    for (const week of weeks) {
      if (!page.text.includes(week.highlight.slice(0, 40))) problems.push(`/changelog/ does not show the highlight of ${week.start}`);
    }
    // Every area starts closed: the page reads as weeks, and opens to the detail.
    if (/<details\b[^>]*\sopen\b/.test(page.body)) problems.push("/changelog/: an area is open; every area starts closed");
    if (!/<details\b/.test(page.body)) problems.push("/changelog/ has no areas under its weeks");
  }
}

// ---------------------------------------------------------------- the feed

const feedFile = path.join(OUT, "changelog", "feed.xml");
if (!fs.existsSync(feedFile)) problems.push("/changelog/feed.xml is missing");
else {
  const feed = fs.readFileSync(feedFile, "utf8");
  if (!/^<\?xml[^>]*\?>\s*<feed xmlns="http:\/\/www\.w3\.org\/2005\/Atom">/.test(feed)) problems.push("/changelog/feed.xml is not an Atom feed");
  const entries = [...feed.matchAll(/<entry>([^]*?)<\/entry>/g)].map((m) => m[1]);
  if (!entries.length && weeks.length) problems.push("/changelog/feed.xml has no entries");
  for (const entry of entries) {
    for (const tag of ["id", "title", "updated", "content"]) if (!new RegExp(`<${tag}[ >]`).test(entry)) problems.push(`/changelog/feed.xml: an entry has no <${tag}>`);
  }
  const first = /<id>[^<]*#week-(\d{4}-\d{2}-\d{2})<\/id>/.exec(entries[0] ?? "")?.[1];
  if (first && weeks.length && first !== weeks[0].start) problems.push(`/changelog/feed.xml: the newest entry is ${first}, changelog.ts says ${weeks[0].start}`);
  if (!all.some((p) => p.head.includes(`href="/changelog/feed.xml"`) || p.head.includes(`href="${SITE}/changelog/feed.xml"`))) {
    problems.push("no page links the feed in its <head>");
  }
}

// --------------------------------------------------------- retired scopes

const root = path.join(WEBSITE, "..", "..");
const workspace = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
const feedText = fs.existsSync(feedFile) ? fs.readFileSync(feedFile, "utf8") : "";
for (const scope of RETIRED_SCOPES) {
  for (const dir of ["apps", "crates"]) {
    if (fs.existsSync(path.join(root, dir, scope))) problems.push(`changelog.ts lists "${scope}" as retired, but ${dir}/${scope} is still in the repository`);
    if (workspace.includes(`"${dir}/${scope}"`)) problems.push(`changelog.ts lists "${scope}" as retired, but Cargo.toml still has ${dir}/${scope} as a member`);
  }
  if (weeks.some((week) => week.areas.some((area) => area.changes.some((change) => change.scope === scope)))) problems.push(`the changelog still lists a change in the retired scope "${scope}"`);
  if (page?.body.includes(`>${scope}</span>`)) problems.push(`/changelog/ shows a change in the retired scope "${scope}"`);
  if (feedText.includes(` (${scope}):`)) problems.push(`/changelog/feed.xml shows a change in the retired scope "${scope}"`);
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
    .filter((n) => typesOf(n).some((t) => t === "WebPage" || t === "CollectionPage" || t === "AboutPage" || t === "TechArticle"))
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
  `changelog with ${weeks.length} week${weeks.length === 1 ? "" : "s"} (newest ${weeks[0]?.start ?? "none"}), its feed, ${RETIRED_SCOPES.length} retired scope${RETIRED_SCOPES.length === 1 ? "" : "s"} left out, ${dated} pages with one git date in the sitemap and the structured data, the IndexNow key in place`,
);
