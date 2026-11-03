import { site } from "../../site.config.ts";
import { latestCommit } from "./last-modified.ts";
import { navreeVersion } from "./version.ts";

/**
 * The facts about Navree that every page, `llms.txt` and the structured data
 * state: one name, one category line, one license, one version, one list of
 * platforms. They are read from `site.config.ts`, the repository's
 * `Cargo.toml` and the CI workflows, never typed twice. `scripts/check-geo.ts`
 * compares what the built site says with this.
 *
 * Relative `.ts` imports only: Node runs this file directly, for the script
 * that writes `llms.txt`.
 */

/** The platforms the test suites run on (`.github/workflows/rust-suites.yml`: ubuntu and windows). */
export const platforms = ["Linux", "Windows"] as const;

export type Fact = { label: string; value: string };

/** The key facts, in the order `llms.txt` lists them. */
export function keyFacts(): Fact[] {
  const version = navreeVersion();
  return [
    { label: "Name", value: site.name },
    { label: "Category", value: site.headline },
    { label: "What it is", value: site.description },
    { label: "License", value: `${site.license} (free and open source, no paid tiers)` },
    { label: "Version", value: `${version} (the first release; from the workspace Cargo.toml)` },
    { label: "Language", value: `Rust ${site.rustVersion} or newer` },
    { label: "Platforms", value: platforms.join(" and ") },
    { label: "Install", value: `\`${site.install.cli}\`, then \`${site.install.newApp}\`` },
    { label: "Repository", value: site.repoUrl },
    { label: "Website and docs", value: `${site.url}/ (every docs page is also Markdown: replace the trailing slash with \`.md\`)` },
    { label: "Last updated", value: latestCommit().slice(0, 10) },
  ];
}

/** The key facts as a Markdown list. */
export function keyFactsMarkdown(): string {
  return keyFacts()
    .map((fact) => `- ${fact.label}: ${fact.value}`)
    .join("\n");
}

/** The site's main pages that are not docs pages, with one line each, for `llms.txt`. */
export const mainPages: { title: string; path: string; summary: string }[] = [
  { title: "Home", path: "/", summary: `What ${site.name} is, what comes in the box, and how to start.` },
  { title: "Navree UI", path: "/ui/", summary: "Typed Rust components rendered on the server: how it works, every component with a live preview, what comes next." },
  { title: "Navree for AI", path: "/ai/", summary: "Agent skills, the Navigator MCP server and docs for models, and the AI SDK for your product." },
  { title: "Architecture", path: "/architecture/", summary: "The system shapes you can build, from a modular monolith to microservices, with one framework." },
  { title: "Ecosystem", path: "/ecosystem/", summary: "The first-party packages and tools: monitoring, admin, queues, debugging, deployment and more." },
  { title: "Performance", path: "/performance/", summary: "Measured results on a CRM-style API, with the workload, the method and how to repeat the run." },
  { title: "Showcase", path: "/showcase/", summary: "The reference CRM with its Next.js and Nuxt front ends, and the first-party tools built on Navree." },
  { title: "Changelog", path: "/changelog/", summary: "Every new feature and fix, by day, written from the commit history at each deploy." },
];
