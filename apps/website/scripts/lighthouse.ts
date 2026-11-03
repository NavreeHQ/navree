/**
 * Runs Lighthouse on the key pages of the built site and holds the scores to
 * a floor. Run after `pnpm build`:
 *
 *   node scripts/lighthouse.ts                 serves out/ itself, checks the key pages
 *   BASE_URL=http://127.0.0.1:3211 node scripts/lighthouse.ts   uses a server that is already running
 *   PAGES=/,/ui/ node scripts/lighthouse.ts    other pages
 *   LIGHTHOUSE_CLI=/path/to/lighthouse/cli/index.js node scripts/lighthouse.ts
 *                                              a Lighthouse you installed, instead of `npx lighthouse@<pinned>`
 *
 * Lighthouse runs with its default mobile settings (a phone-sized screen, a
 * slow 4G connection and a 4x slower CPU). The floors are for catching a
 * collapse, not for setting a goal: SEO and accessibility must stay at or
 * above 0.95, best practices at or above 0.9, layout shift at or below 0.1,
 * and performance at or above 0.25 (CI runners are slow and vary run to
 * run). The scores, LCP, blocking time and transfer size of every page are
 * printed and written to `report/lighthouse.md`.
 */
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

import { WEBSITE, finish } from "./lib/site-html.ts";

/** Lighthouse version, pinned. Update it on purpose, not by a floating tag. */
const LIGHTHOUSE = "lighthouse@13.5.0";
const DEFAULT_PAGES = ["/", "/ui/", "/docs/getting-started/installation/", "/docs/components/button/"];
const FLOOR = { performance: 0.25, accessibility: 0.95, "best-practices": 0.9, seo: 0.95 } as const;
const MAX_CLS = 0.1;

const pagesToCheck = (process.env.PAGES ?? DEFAULT_PAGES.join(",")).split(",").map((p) => p.trim()).filter(Boolean);
const REPORT = path.join(WEBSITE, "report");
fs.mkdirSync(REPORT, { recursive: true });

const ready = (url: string) =>
  new Promise<boolean>((resolve) => {
    http
      .get(url, (res) => {
        res.resume();
        resolve((res.statusCode ?? 500) < 500);
      })
      .on("error", () => resolve(false));
  });

let server: ChildProcess | null = null;
let base = process.env.BASE_URL ?? "";
if (!base) {
  const port = String(30000 + Math.floor(Math.random() * 20000));
  base = `http://127.0.0.1:${port}`;
  server = spawn(process.execPath, [path.join(WEBSITE, "scripts", "serve.ts")], { env: { ...process.env, PORT: port }, stdio: "ignore" });
  for (let i = 0; i < 50 && !(await ready(`${base}/`)); i++) await new Promise((r) => setTimeout(r, 200));
  if (!(await ready(`${base}/`))) {
    server.kill();
    console.error("lighthouse: the static server did not start; run `pnpm build` first");
    process.exit(1);
  }
}

const problems: string[] = [];
const rows: string[] = [];
const cli = process.env.LIGHTHOUSE_CLI ? [process.execPath, process.env.LIGHTHOUSE_CLI] : ["npx", "--yes", LIGHTHOUSE];

try {
  for (const page of pagesToCheck) {
    const name = `lighthouse-${page.replace(/\//g, "_") || "home"}.json`;
    const output = path.join(REPORT, name);
    const run = spawnSync(
      cli[0],
      [
        ...cli.slice(1),
        `${base}${page}`,
        "--only-categories=performance,accessibility,best-practices,seo",
        "--chrome-flags=--headless=new --no-sandbox",
        "--output=json",
        `--output-path=${output}`,
        "--quiet",
      ],
      { encoding: "utf8", shell: process.platform === "win32", timeout: 5 * 60 * 1000 },
    );
    if (run.status !== 0 || !fs.existsSync(output)) {
      problems.push(`${page}: Lighthouse did not finish (${(run.stderr || run.stdout || "no output").trim().split("\n").slice(-3).join(" | ")})`);
      continue;
    }
    const lhr = JSON.parse(fs.readFileSync(output, "utf8")) as {
      categories: Record<string, { score: number | null }>;
      audits: Record<string, { numericValue?: number; displayValue?: string }>;
      lighthouseVersion: string;
    };
    const score = (id: keyof typeof FLOOR) => lhr.categories[id]?.score ?? 0;
    for (const id of Object.keys(FLOOR) as (keyof typeof FLOOR)[]) {
      if (score(id) < FLOOR[id]) problems.push(`${page}: ${id} is ${Math.round(score(id) * 100)}, the floor is ${Math.round(FLOOR[id] * 100)}`);
    }
    const cls = lhr.audits["cumulative-layout-shift"]?.numericValue ?? 0;
    if (cls > MAX_CLS) problems.push(`${page}: layout shift is ${cls.toFixed(3)}, the limit is ${MAX_CLS}`);
    const show = (id: string) => lhr.audits[id]?.displayValue?.replace(/ /g, " ") ?? "n/a";
    rows.push(
      `| ${page} | ${Math.round(score("performance") * 100)} | ${Math.round(score("accessibility") * 100)} | ${Math.round(score("best-practices") * 100)} | ${Math.round(score("seo") * 100)} | ${show("largest-contentful-paint")} | ${show("cumulative-layout-shift")} | ${show("total-blocking-time")} | ${show("total-byte-weight").replace("Total size was ", "")} |`,
    );
  }
} finally {
  server?.kill();
}

const table = [
  "| Page | Performance | Accessibility | Best practices | SEO | LCP | CLS | Blocking time | Transfer |",
  "|---|---|---|---|---|---|---|---|---|",
  ...rows,
].join("\n");
fs.writeFileSync(path.join(REPORT, "lighthouse.md"), `${table}\n`);
console.log(table);

finish("lighthouse", problems, `${pagesToCheck.length} pages at or above the floors (performance ${FLOOR.performance * 100}, accessibility ${FLOOR.accessibility * 100}, best practices ${FLOOR["best-practices"] * 100}, SEO ${FLOOR.seo * 100}, layout shift at most ${MAX_CLS})`);
