import fs from "node:fs";
import path from "node:path";

import { agentPages, llmsTxt } from "@/lib/agent-markdown";
import { listSkills } from "@/lib/agent-skills";

/*
 * What the /ai/ page reads at build time: the agent skills in `skills/`,
 * the start of the real `llms.txt`, and the start of the `AGENTS.md` that
 * `navree new` writes (from the CLI's template).
 */

export function skillsData() {
  const skills = listSkills();
  return {
    skills: skills.map((s) => ({ name: s.name, label: s.label, path: s.path })),
    files: skills.reduce((n, s) => n + s.files.length, 0),
  };
}

/** The first lines of `/llms.txt`, as the site publishes it. */
export function llmsPreview(lines = 24): { text: string; pages: number } {
  const pages = agentPages();
  const skills = listSkills().map((s) => ({ name: s.name, summary: s.summary, url: s.url }));
  return { text: llmsTxt(pages, skills).split("\n").slice(0, lines).join("\n"), pages: pages.length };
}

const FALLBACK_AGENTS_MD = `# AGENTS.md

Instructions for coding agents working on crm, a Navree app: a Rust
backend built from modules wired by dependency injection. Read this file
before changing code.

## Layout

\`\`\`text
config/app.toml        settings shared by every environment (committed)
.env                   this machine's secrets, such as APP_KEY (never committed)
src/app.rs             the root module: every module, configured once
src/modules/<name>/    feature modules: mod.rs, controller.rs, service.rs, ...
tests/                 tests that boot the whole app
\`\`\`

## Commands

\`\`\`sh
navree dev            # serve on http://127.0.0.1:8080, rebuild on save
cargo test            # every test; each gets its own SQLite database
navree check          # validate the module graph without booting
\`\`\``;

/**
 * The start of the `AGENTS.md` that `navree new crm` writes: the CLI's
 * template with the app's name filled in and the optional parts left out.
 */
export function agentsMdExcerpt(lines = 30): string {
  const template = path.resolve(process.cwd(), "..", "..", "crates", "navree-cli", "templates", "app", "AGENTS.md.j2");
  let text = FALLBACK_AGENTS_MD;
  try {
    text = fs
      .readFileSync(template, "utf8")
      .replace(/\r\n?/g, "\n")
      // Optional blocks (`{% if ui_next %}`) are not in a default app.
      .replace(/\{%-?\s*if [^%]*%\}[\s\S]*?\{%-?\s*endif\s*-?%\}\n?/g, "")
      .replace(/\{%[^%]*%\}\n?/g, "")
      .replace(/\{\{-?\s*(title|name|app_name|package)\s*-?\}\}/g, "crm")
      .replace(/\{\{[^}]*\}\}/g, "crm");
  } catch {
    // The template moved: show the fallback.
  }
  return text.split("\n").slice(0, lines).join("\n").trimEnd();
}
