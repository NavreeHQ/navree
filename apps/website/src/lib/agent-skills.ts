/**
 * The agent skills in the repository's `skills/` directory: one folder per
 * skill with a `SKILL.md` (YAML front-matter `name` and `description`) and
 * optional reference files.
 *
 * Read at build time by `scripts/agent-files.ts` (which publishes them at
 * `/skills/<name>/SKILL.md` with `/skills/index.json`) and by the `/ai/`
 * page. Whatever exists when the site is built is what gets served. Node
 * runs the script with the types stripped, so this module keeps to
 * relative `.ts` imports and erasable TypeScript.
 */
import fs from "node:fs";
import path from "node:path";

import matter from "gray-matter";

import { site } from "../../site.config.ts";

export type Skill = {
  /** The folder name, which is also the URL segment. */
  name: string;
  /** The full front-matter description: what it covers and when to use it. */
  description: string;
  /** The description's first sentence, for lists. */
  summary: string;
  /** The summary up to its first colon, for compact lists. */
  label: string;
  /** `https://navree.com/skills/<name>/SKILL.md` */
  url: string;
  /** `/skills/<name>/SKILL.md` */
  path: string;
  /** Every file of the skill, relative to its folder, `SKILL.md` first. */
  files: string[];
  /** The folder on disk. */
  dir: string;
};

/** `<repository>/skills`, from `apps/website`. */
export function skillsDir(): string {
  return path.resolve(process.cwd(), "..", "..", "skills");
}

function filesOf(dir: string, prefix = ""): string[] {
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((entry) => !entry.name.startsWith("."))
    .flatMap((entry) => {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      return entry.isDirectory() ? filesOf(path.join(dir, entry.name), rel) : [rel];
    });
}

/** The first sentence of a description that says what the skill covers (not "Use when ..."). */
function summarize(description: string): string {
  const sentences = description.split(/(?<=[.!?])\s+(?=[A-Z`])/);
  return sentences.find((s) => !/^Use (it )?when\b/i.test(s)) ?? sentences[0] ?? description;
}

/** The part of a summary before its first colon: "Adds HTTP endpoints to a Navree app". */
function labelOf(summary: string): string {
  const colon = summary.indexOf(": ");
  return (colon > 0 ? summary.slice(0, colon) : summary).replace(/\.$/, "");
}

/** Every skill, the umbrella `navree` first, then by name. */
export function listSkills(dir: string = skillsDir()): Skill[] {
  if (!fs.existsSync(dir)) return [];
  const skills: Skill[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name.startsWith(".")) continue;
    const folder = path.join(dir, entry.name);
    const file = path.join(folder, "SKILL.md");
    if (!fs.existsSync(file)) continue;
    let data: Record<string, unknown> = {};
    try {
      data = matter(fs.readFileSync(file, "utf8")).data;
    } catch {
      // A skill being written can have broken front-matter; list it by name.
    }
    const description = String(data.description ?? "").replace(/\s+/g, " ").trim();
    const files = filesOf(folder).sort((a, b) => (a === "SKILL.md" ? -1 : b === "SKILL.md" ? 1 : a.localeCompare(b)));
    const summary = summarize(description);
    skills.push({
      name: entry.name,
      description,
      summary,
      label: labelOf(summary),
      url: `${site.url}/skills/${entry.name}/SKILL.md`,
      path: `/skills/${entry.name}/SKILL.md`,
      files,
      dir: folder,
    });
  }
  return skills.sort((a, b) => (a.name === "navree" ? -1 : b.name === "navree" ? 1 : a.name.localeCompare(b.name)));
}

/** The first lines of a skill's `SKILL.md`, for previews. */
export function skillHead(skill: Skill, lines = 18): string {
  const text = fs.readFileSync(path.join(skill.dir, "SKILL.md"), "utf8").replace(/\r\n?/g, "\n");
  return text.split("\n").slice(0, lines).join("\n").trimEnd();
}
