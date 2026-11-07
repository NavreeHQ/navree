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
 * The pages are served by `scripts/serve.ts`, which sends text compressed
 * with Brotli as the live site's CDN does; a server that does not compress
 * gives scores 15 to 20 points too low. The script asks the server for the
 * home page with `Accept-Encoding: br, gzip`, names the answer in the report
 * and warns when the server does not compress.
 *
 * Lighthouse runs with its default mobile settings (a phone-sized screen, a
 * slow 4G connection and a 4x slower CPU). The floors are for catching a
 * collapse, not for setting a goal: SEO and accessibility must stay at or
 * above 0.95, best practices at or above 0.9, layout shift at or below 0.1,
 * and performance at or above 0.55. The docs pages score 95 to 99 on the
 * pinned Lighthouse, which estimates a phone's load from the trace of one run;
 * the home page scores 63 to 72 on CI's runners, held back by the time its
 * scripts block the main thread (0.85 to 2.6 s from run to run). The floor
 * sits under the home page's range, so it catches a collapse anywhere without
 * failing on the home page's known cost. The scores, LCP, blocking
 * time and transfer size of every page are printed and written to
 * `report/lighthouse.md`.
 */
import { spawnSync, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

import { serveOut } from "./lib/serve-out.ts";
import { WEBSITE, finish } from "./lib/site-html.ts";

/** Lighthouse version, pinned. Update it on purpose, not by a floating tag. */
const LIGHTHOUSE = "lighthouse@13.5.0";
const DEFAULT_PAGES = ["/", "/ui/", "/docs/getting-started/installation/", "/docs/components/button/"];
const FLOOR = { performance: 0.55, accessibility: 0.95, "best-practices": 0.9, seo: 0.95 } as const;
const MAX_CLS = 0.1;

const pagesToCheck = (process.env.PAGES ?? DEFAULT_PAGES.join(",")).split(",").map((p) => p.trim()).filter(Boolean);
const REPORT = path.join(WEBSITE, "report");
fs.mkdirSync(REPORT, { recursive: true });

/** The `Content-Encoding` the server answers with when the client accepts Brotli and gzip ("none" when it sends the file as is). */
const encodingOf = (url: string) =>
  new Promise<string>((resolve) => {
    http
      .get(url, { headers: { "Accept-Encoding": "br, gzip" } }, (res) => {
        res.resume();
        resolve(String(res.headers["content-encoding"] ?? "none"));
      })
      .on("error", () => resolve("unknown"));
  });

let server: ChildProcess | null = null;
let base = process.env.BASE_URL ?? "";
if (!base) {
  ({ base, server } = await serveOut("lighthouse"));
}

const encoding = await encodingOf(`${base}/`);
const compressing = encoding === "br" || encoding === "gzip";
if (!compressing) {
  console.warn(
    `lighthouse: ${base} sends text uncompressed (Content-Encoding: ${encoding}); the scores will be lower than the live site's. Use scripts/serve.ts, or a server that sends Brotli or gzip.`,
  );
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
      // `npx` is a script on Windows and needs a shell; a Node path with spaces must not go through one.
      { encoding: "utf8", shell: process.platform === "win32" && cli[0] === "npx", timeout: 5 * 60 * 1000 },
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
const served = compressing
  ? `Served with ${encoding === "br" ? "Brotli" : "gzip"}, as the live site is.`
  : `Served without compression (Content-Encoding: ${encoding}): the scores are lower than the live site's.`;
fs.writeFileSync(path.join(REPORT, "lighthouse.md"), `${table}\n\n${served}\n`);
console.log(table);

finish("lighthouse", problems, `${pagesToCheck.length} pages at or above the floors (performance ${FLOOR.performance * 100}, accessibility ${FLOOR.accessibility * 100}, best practices ${FLOOR["best-practices"] * 100}, SEO ${FLOOR.seo * 100}, layout shift at most ${MAX_CLS})`);
