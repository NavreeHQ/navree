/**
 * Checks what AI crawlers and assistants read: run after `pnpm build`.
 *
 *   node scripts/check-geo.ts
 *
 * - `robots.txt` names every AI crawler in `src/lib/crawlers.ts` and allows it;
 * - `llms.txt` follows the llms.txt format (an H1, a block-quote summary, H2
 *   sections of `- [title](url): note` lines, an Optional section last), has
 *   the key facts, links every docs page as Markdown, and every link in it
 *   resolves to a file of the export;
 * - `llms-full.txt` opens with the same key facts and holds every docs page;
 * - every docs page links its Markdown copy with
 *   `<link rel="alternate" type="text/markdown">`, and the copy exists;
 * - one name, one category line, one license, one version, one list of
 *   platforms: the homepage, the structured data, `llms.txt` and
 *   `llms-full.txt` all say what `site.config.ts`, the workspace `Cargo.toml`
 *   and `src/lib/site-facts.ts` say, and no page uses an old name of the
 *   project or a 0.x version of Navree.
 */
import fs from "node:fs";
import path from "node:path";

import { site } from "../site.config.ts";
import { aiCrawlers } from "../src/lib/crawlers.ts";
import { keyFacts, keyFactsMarkdown, platforms } from "../src/lib/site-facts.ts";
import { navreeVersion } from "../src/lib/version.ts";
import { OUT, SITE, WEBSITE, finish, indexable, nodes, pages, requireOut, typesOf } from "./lib/site-html.ts";

requireOut("check-geo");
process.chdir(WEBSITE);

const problems: string[] = [];
const read = (file: string) => fs.readFileSync(path.join(OUT, file), "utf8");
const all = pages();
const version = navreeVersion();

// ------------------------------------------------------------ robots.txt

const robots = read("robots.txt");
const groups = robots
  .split(/\n\s*\n/)
  .map((block) => block.split("\n").map((line) => line.trim()))
  .map((lines) => ({
    agents: lines.filter((l) => /^user-agent:/i.test(l)).map((l) => l.replace(/^user-agent:\s*/i, "").toLowerCase()),
    allows: lines.filter((l) => /^allow:/i.test(l)).map((l) => l.replace(/^allow:\s*/i, "")),
    disallows: lines.filter((l) => /^disallow:\s*\S/i.test(l)),
  }));
for (const crawler of aiCrawlers) {
  const group = groups.find((g) => g.agents.includes(crawler.toLowerCase()));
  if (!group) problems.push(`robots.txt does not name ${crawler}`);
  else if (!group.allows.includes("/") || group.disallows.length) problems.push(`robots.txt does not allow ${crawler} everywhere`);
}

// -------------------------------------------------------------- llms.txt

const llms = read("llms.txt");
const lines = llms.split("\n");
if (lines[0] !== `# ${site.name}`) problems.push(`llms.txt: the first line must be "# ${site.name}", got "${lines[0]}"`);
const summary = lines.find((l, i) => i > 0 && l.trim());
if (!summary?.startsWith("> ")) problems.push("llms.txt: the first paragraph after the title must be a block-quote summary");
else if (summary.length > 400) problems.push(`llms.txt: the summary is ${summary.length} characters; keep it crisp (400 at most)`);
if (lines.filter((l) => /^# /.test(l)).length !== 1) problems.push("llms.txt: exactly one H1 allowed");
const headings = lines.filter((l) => /^## /.test(l));
if (headings.at(-1) !== "## Optional") problems.push(`llms.txt: the last section must be "## Optional", got "${headings.at(-1)}"`);
for (const wanted of ["## Key facts", "## Main pages"]) if (!headings.includes(wanted)) problems.push(`llms.txt: no "${wanted}" section`);
const linkLine = /^- \[[^\]]+\]\(https:\/\/navree\.com\/[^)\s]*\)(: .+)?$/;
let inFacts = false;
for (const [i, line] of lines.entries()) {
  if (/^## /.test(line)) inFacts = line === "## Key facts";
  else if (line.startsWith("- ") && !inFacts && !linkLine.test(line)) problems.push(`llms.txt:${i + 1}: not a "- [title](url): note" line: ${line.slice(0, 90)}`);
  if (line.length > 700) problems.push(`llms.txt:${i + 1}: a line of ${line.length} characters; cut it to a sentence`);
}
const facts = keyFactsMarkdown();
if (!llms.includes(facts)) problems.push("llms.txt: the Key facts block differs from src/lib/site-facts.ts");

// Every link in llms.txt is a file of the export.
const linked = new Set<string>();
for (const m of llms.matchAll(/\]\((https:\/\/navree\.com\/[^)\s]*)\)/g)) linked.add(m[1].replace(SITE, ""));
const exists = (url: string) => {
  const clean = url.split("#")[0];
  const file = path.join(OUT, clean);
  return (fs.existsSync(file) && fs.statSync(file).isFile()) || fs.existsSync(path.join(file, "index.html"));
};
for (const url of linked) if (!exists(url)) problems.push(`llms.txt links ${url}, which is not in the export`);

// Every docs page is linked as Markdown.
const docPages = all.filter((p) => p.url.startsWith("/docs/") && p.url !== "/docs/" && !p.noindex);
for (const page of docPages) {
  const md = `${page.url.replace(/\/$/, "")}.md`;
  if (!linked.has(md)) problems.push(`llms.txt does not link ${md}`);
}

// ---------------------------------------------------------- llms-full.txt

const full = read("llms-full.txt");
if (!full.startsWith(`# ${site.name} documentation\n`)) problems.push("llms-full.txt: must start with the documentation title");
if (!full.includes(facts)) problems.push("llms-full.txt: the Key facts block differs from src/lib/site-facts.ts");
for (const page of docPages) if (!full.includes(`URL: ${SITE}${page.url}`)) problems.push(`llms-full.txt does not hold ${page.url}`);

// ---------------------------------------------- Markdown copies of docs pages

for (const page of docPages) {
  const md = `${page.url.replace(/\/$/, "")}.md`;
  const alternate = page.head.includes(`<link rel="alternate" type="text/markdown" href="${SITE}${md}"`);
  if (!alternate) problems.push(`${page.url}: no <link rel="alternate" type="text/markdown" href="${SITE}${md}">`);
  if (!fs.existsSync(path.join(OUT, md))) problems.push(`${page.url}: the Markdown copy ${md} is not in the export`);
}

// ------------------------------------------------------ facts everywhere

const home = all.find((p) => p.url === "/");
if (!home) problems.push("the homepage is missing");
else {
  if (!home.title.startsWith(`${site.name}: ${site.headline}`)) problems.push(`homepage title is "${home.title}", expected it to start with "${site.name}: ${site.headline}"`);
  const category = site.headline.replace(/^The /, "").replace(/\.$/, "");
  if (!home.text.includes(category)) problems.push(`the homepage text does not state the category line ("${category}")`);
}

const graph = new Map<string, Record<string, unknown>>();
for (const page of indexable(all)) {
  for (const node of nodes(page)) for (const type of typesOf(node)) if (!graph.has(type)) graph.set(type, node);
}
const need = (type: string, key: string, expected: unknown) => {
  const got = graph.get(type)?.[key];
  if (JSON.stringify(got) !== JSON.stringify(expected)) problems.push(`JSON-LD ${type}.${key} is ${JSON.stringify(got)}, expected ${JSON.stringify(expected)}`);
};
need("Organization", "name", site.name);
need("Organization", "slogan", site.headline);
need("WebSite", "name", site.name);
need("SoftwareApplication", "name", site.name);
need("SoftwareApplication", "softwareVersion", version);
need("SoftwareApplication", "operatingSystem", platforms.join(", "));
need("SoftwareApplication", "isAccessibleForFree", true);
need("SoftwareApplication", "softwareRequirements", `Rust ${site.rustVersion} or newer`);
need("SoftwareSourceCode", "version", version);
need("SoftwareSourceCode", "codeRepository", site.repoUrl);
const expectedLicenses = site.license.split(" OR ").map((id) => ({ MIT: "https://opensource.org/licenses/MIT", "Apache-2.0": "https://www.apache.org/licenses/LICENSE-2.0" })[id as "MIT"]);
need("SoftwareApplication", "license", expectedLicenses);
need("SoftwareSourceCode", "license", expectedLicenses);
const sameAs = (graph.get("Organization")?.sameAs ?? []) as string[];
if (!sameAs.some((url) => new URL(url).pathname.split("/").filter(Boolean).length === 1 && url.startsWith("https://github.com/"))) {
  problems.push("JSON-LD Organization.sameAs does not list the GitHub organisation");
}

// The facts block says the same.
for (const fact of keyFacts()) {
  if (fact.label === "Version" && !fact.value.startsWith(version)) problems.push(`key facts say version "${fact.value}", Cargo.toml says ${version}`);
  if (fact.label === "License" && !fact.value.startsWith(site.license)) problems.push(`key facts say license "${fact.value}", site.config.ts says ${site.license}`);
}

// No page names the project's old names or a 0.x release.
for (const page of all) {
  if (page.url === "/styleguide/") continue;
  for (const [pattern, why] of [
    // The old names, written in pieces so that this file never spells them.
    [new RegExp(`\\b(?:${["Ke" + "el", "Na" + "vis"].join("|")})\\b`), "an old name"],
    [/\bNavree v?0\.\d+(?:\.\d+)?\b/, "a 0.x version of Navree (the first release is 1.0)"],
  ] as [RegExp, string][]) {
    const hit = pattern.exec(page.text);
    if (hit) problems.push(`${page.url}: says "${hit[0]}", ${why}`);
  }
}

finish(
  "check-geo",
  problems,
  `${aiCrawlers.length} AI crawlers allowed; llms.txt (${linked.size} links) and llms-full.txt follow the format; ${docPages.length} docs pages link their Markdown copy; name, category, license ${site.license}, version ${version} and platforms agree everywhere`,
);
