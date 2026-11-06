/**
 * The daily site check, the same steps on a laptop and in CI
 * (`.github/workflows/site-daily.yml`):
 *
 *   node scripts/site-daily.ts                 every step
 *   SKIP=lint,typecheck node scripts/site-daily.ts   leave steps out
 *   SKIP_BUILD=1 node scripts/site-daily.ts    use the `out/` that is already built
 *
 * Steps, in order: lint, typecheck, build, then the checks on the built
 * site (links, SEO, GEO, AEO, freshness, the 404 page) and Lighthouse on the key pages.
 * A failing step does not stop the ones after it, except the build, which
 * the checks need. Each step's output is kept in `report/<step>.log`, and
 * `report/summary.md` says what passed, what failed and why: the workflow
 * puts that file in the GitHub issue it opens. Exits with 1 when any step
 * failed.
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { WEBSITE } from "./lib/site-html.ts";

type Step = { name: string; command: string[]; fatal?: boolean; skipWhen?: boolean };

const skip = new Set((process.env.SKIP ?? "").split(",").filter(Boolean));
const steps: Step[] = [
  { name: "lint", command: ["pnpm", "lint"] },
  { name: "typecheck", command: ["pnpm", "typecheck"] },
  { name: "build", command: ["pnpm", "build"], fatal: true, skipWhen: process.env.SKIP_BUILD === "1" },
  { name: "check:links", command: ["pnpm", "check:links"] },
  { name: "check:seo", command: ["pnpm", "check:seo"] },
  { name: "check:geo", command: ["pnpm", "check:geo"] },
  { name: "check:aeo", command: ["pnpm", "check:aeo"] },
  { name: "check:fresh", command: ["pnpm", "check:fresh"] },
  { name: "check:404", command: ["pnpm", "check:404"] },
  { name: "lighthouse", command: ["node", "scripts/lighthouse.ts"] },
];

const REPORT = path.join(WEBSITE, "report");
fs.rmSync(REPORT, { recursive: true, force: true });
fs.mkdirSync(REPORT, { recursive: true });

const results: { name: string; ok: boolean; seconds: number; tail: string }[] = [];
let stopped = false;
for (const step of steps) {
  if (skip.has(step.name) || step.skipWhen) {
    console.log(`\n== ${step.name}: skipped`);
    continue;
  }
  if (stopped) {
    results.push({ name: step.name, ok: false, seconds: 0, tail: "Not run: the build failed." });
    continue;
  }
  console.log(`\n== ${step.name}`);
  const started = Date.now();
  const run = spawnSync(step.command[0], step.command.slice(1), {
    cwd: WEBSITE,
    encoding: "utf8",
    shell: process.platform === "win32",
    maxBuffer: 256 * 1024 * 1024,
    env: { ...process.env, FORCE_COLOR: "0", NO_COLOR: "1" },
  });
  const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
  fs.writeFileSync(path.join(REPORT, `${step.name.replace(":", "-")}.log`), output);
  const ok = run.status === 0;
  process.stdout.write(output.split("\n").slice(-12).join("\n"));
  results.push({ name: step.name, ok, seconds: Math.round((Date.now() - started) / 1000), tail: output.trim().split("\n").slice(-30).join("\n") });
  if (!ok && step.fatal) stopped = true;
}

const failed = results.filter((r) => !r.ok);
const sha = process.env.GITHUB_SHA?.slice(0, 7) ?? spawnSync("git", ["rev-parse", "--short", "HEAD"], { cwd: WEBSITE, encoding: "utf8" }).stdout.trim();
const runUrl = process.env.GITHUB_SERVER_URL && process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}` : "";
const lighthouse = fs.existsSync(path.join(REPORT, "lighthouse.md")) ? fs.readFileSync(path.join(REPORT, "lighthouse.md"), "utf8") : "";
const summary = [
  failed.length ? `The daily site check failed on ${failed.length} of ${results.length} steps.` : `The daily site check passed: ${results.length} steps.`,
  "",
  `Commit \`${sha}\`${runUrl ? `, [run](${runUrl})` : ""}, ${new Date().toISOString().slice(0, 10)}.`,
  "",
  "| Step | Result | Seconds |",
  "|---|---|---|",
  ...results.map((r) => `| ${r.name} | ${r.ok ? "passed" : "FAILED"} | ${r.seconds} |`),
  ...failed.flatMap((r) => ["", `### ${r.name}`, "", "```text", r.tail, "```"]),
  ...(lighthouse ? ["", "### Lighthouse (mobile)", "", lighthouse] : []),
  "",
].join("\n");
fs.writeFileSync(path.join(REPORT, "summary.md"), summary);
console.log(`\n${summary.split("\n").slice(0, 3).join("\n")}\nReport: ${path.relative(process.cwd(), path.join(REPORT, "summary.md"))}`);
process.exit(failed.length ? 1 : 0);
