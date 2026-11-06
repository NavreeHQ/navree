/**
 * Writes the docs for agents and the served agent skills into `public/`,
 * so `next dev` serves them and `next build` copies them into `out/`:
 *
 * - `public/docs/<section>/<page>.md`: every docs page as Markdown
 *   (`src/lib/agent-markdown.ts` says how components are converted), and
 *   `public/docs/<section>.md`: each section's own page, which lists its pages;
 * - `public/llms.txt`: the index, one line per page linking its `.md`;
 * - `public/llms-full.txt`: every page in one file, and `public/llms-full/<section>.txt`:
 *   the pages of one section in one file;
 * - `public/skills/<name>/...`: each skill of the repository's `skills/`
 *   directory (its `SKILL.md` and reference files), as they are at build
 *   time;
 * - `public/skills/index.json`: every skill's name, description and URL;
 * - `public/v/<version>/docs.json`: every page split at its `##` headings,
 *   for Navigator (`navree mcp`), which downloads the docs of the Navree
 *   version an app uses (`<version>` is the workspace's, from the
 *   repository's `Cargo.toml`).
 *
 * Runs before every `dev` and `build` (package.json), after
 * `scripts/prebuild.ts` has checked the docs. Everything it writes is
 * generated and git-ignored. Run it alone with `node scripts/agent-files.ts`.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { agentPages, agentSections, docsBundle, llmsFullTxt, llmsSectionTxt, llmsTxt, pageMarkdown, sectionFullPath, sectionMarkdown } from "../src/lib/agent-markdown.ts";
import { listSkills } from "../src/lib/agent-skills.ts";
import { site } from "../site.config.ts";

const WEBSITE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PUBLIC = path.join(WEBSITE, "public");

// The libraries resolve `content/docs` and `../../skills` from here.
process.chdir(WEBSITE);

function write(file: string, contents: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, contents);
}

const started = performance.now();

for (const generated of ["docs", "skills", "llms.txt", "llms-full.txt", "llms-full", "v"]) {
  fs.rmSync(path.join(PUBLIC, generated), { recursive: true, force: true });
}

// Docs pages as Markdown.
const pages = agentPages();
const markdown = new Map<string, string>();
const problems: string[] = [];
for (const page of pages) {
  try {
    const md = pageMarkdown(page);
    markdown.set(page.mdPath, md);
    write(path.join(PUBLIC, ...page.mdPath.split("/").filter(Boolean)), md);
  } catch (error) {
    problems.push(`content/docs/${page.section}/${page.page}.mdx: ${(error as Error).message}`);
  }
}
if (problems.length) {
  console.error(`\nagent-files: these pages could not be converted to Markdown:\n  - ${problems.join("\n  - ")}\n`);
  process.exit(1);
}

// Each section's own page as Markdown: `public/docs/<section>.md`.
const sectionPages = agentSections(pages);
for (const section of sectionPages) {
  write(path.join(PUBLIC, ...section.mdPath.split("/").filter(Boolean)), sectionMarkdown(section));
}

// The skills, copied as they are, and their index.
const skills = listSkills();
for (const skill of skills) {
  for (const file of skill.files) {
    const target = path.join(PUBLIC, "skills", skill.name, ...file.split("/"));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(path.join(skill.dir, ...file.split("/")), target);
  }
}
const index = {
  name: `${site.name} agent skills`,
  description:
    "Skills that teach coding agents how to build with Navree, one per common task. Install them with `navree skills:install` or `npx skills add NavreeHQ/navree`.",
  docs: `${site.url}/docs/ai/agent-skills/`,
  skills: skills.map((skill) => ({
    name: skill.name,
    description: skill.description,
    url: skill.url,
    files: skill.files,
  })),
};
write(path.join(PUBLIC, "skills", "index.json"), `${JSON.stringify(index, null, 2)}\n`);

// The llms files.
write(path.join(PUBLIC, "llms.txt"), llmsTxt(pages, skills));
const full = llmsFullTxt(pages, markdown);
write(path.join(PUBLIC, "llms-full.txt"), full);
for (const section of sectionPages) {
  write(path.join(PUBLIC, ...sectionFullPath(section.slug).split("/").filter(Boolean)), llmsSectionTxt(section, markdown));
}

// Navigator's docs bundle for this version.
const cargoToml = fs.readFileSync(path.join(WEBSITE, "..", "..", "Cargo.toml"), "utf8");
const version = /\[workspace\.package\][^[]*?\bversion\s*=\s*"([^"]+)"/.exec(cargoToml)?.[1];
if (!version) {
  console.error("agent-files: no [workspace.package] version in the repository's Cargo.toml");
  process.exit(1);
}
write(path.join(PUBLIC, "v", version, "docs.json"), `${JSON.stringify(docsBundle(version, pages))}\n`);

console.log(
  `agent-files: ${pages.length} Markdown pages and ${sectionPages.length} sections, llms.txt, llms-full.txt (${Math.round(full.length / 1024)} KB) and ${sectionPages.length} section files, ` +
    `docs.json for ${version}, ${skills.length} skills (${Math.round(performance.now() - started)} ms)`,
);
