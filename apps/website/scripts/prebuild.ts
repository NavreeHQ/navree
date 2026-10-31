/**
 * Runs before every `dev`, `build` and `typecheck` (Node strips the types;
 * no build step). It generates everything the site reads at build time:
 *
 * - `src/generated/performance.json`: Navree's own benchmark results, from
 *   `bench/results/navree.json`, the repeat run in
 *   `bench/results/extra/navree-control.json`, `bench/results/env.json` and
 *   the README of Navree's benchmark app (`bench/frameworks/navree`).
 * - `public/data/bench/`: those two result files, for download.
 * - `public/data/search-index.json`: the docs search index (MiniSearch).
 *
 * It also checks that every page in `content/docs/manifest.ts` exists and
 * has a title and a description, and fails the build if one doesn't.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { createProcessor } from "@mdx-js/mdx";
import GithubSlugger from "github-slugger";
import matter from "gray-matter";
import MiniSearch from "minisearch";
import remarkGfm from "remark-gfm";

import { sections, links } from "../content/docs/manifest.ts";
import type { CpuSet, PerfData, StackRow, TestInfo, TestKey, TestResult } from "../src/lib/bench-types.ts";
import { searchOptions, type SearchDoc } from "../src/lib/search-options.ts";

const WEBSITE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const REPO = path.resolve(WEBSITE, "../..");
const BENCH = path.join(REPO, "bench");
const RESULTS = path.join(BENCH, "results");
const DOCS = path.join(WEBSITE, "content/docs");
const GENERATED = path.join(WEBSITE, "src/generated");
const PUBLIC_DATA = path.join(WEBSITE, "public/data");

// ------------------------------------------------------------------ helpers

function readJson<T>(file: string): T {
  return JSON.parse(fs.readFileSync(file, "utf8")) as T;
}

function writeFile(file: string, contents: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, contents);
}

function fail(message: string): never {
  console.error(`\nprebuild: ${message}\n`);
  process.exit(1);
}

/** "0-7" → 8 CPUs, "0,2,4" → 3. */
function cpuSet(range: string): CpuSet {
  let count = 0;
  for (const part of range.split(",")) {
    const [a, b] = part.split("-").map((n) => Number.parseInt(n, 10));
    count += b === undefined ? 1 : b - a + 1;
  }
  return { range, count };
}

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** A README table cell (inline Markdown) as safe HTML: code, bold, links. */
function inlineMarkdown(md: string): string {
  const parts = md.split(/(`[^`]*`)/g);
  return parts
    .map((part) => {
      if (part.startsWith("`") && part.endsWith("`") && part.length >= 2) {
        return `<code>${escapeHtml(part.slice(1, -1))}</code>`;
      }
      return escapeHtml(part)
        .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
        .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2">$1</a>');
    })
    .join("");
}

function splitRow(line: string): string[] {
  const trimmed = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return trimmed.split(/(?<!\\)\|/).map((cell) => cell.trim().replace(/\\\|/g, "|"));
}

type ReadmeInfo = { title: string; tables: { header: string[]; rows: string[][] }[] };

/** The title and every table of a README. */
function parseReadme(file: string): ReadmeInfo {
  const text = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "";
  const lines = text.split(/\r?\n/);
  const title = (lines.find((l) => l.startsWith("# ")) ?? "").replace(/^# /, "").trim();
  const tables: ReadmeInfo["tables"] = [];
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].trim().startsWith("|") && lines[i + 1]?.trim().match(/^\|[\s|:-]+\|$/)) {
      const header = splitRow(lines[i]);
      const rows: string[][] = [];
      let j = i + 2;
      while (j < lines.length && lines[j].trim().startsWith("|")) rows.push(splitRow(lines[j++]));
      tables.push({ header, rows });
      i = j - 1;
    }
  }
  return { title, tables };
}

/**
 * The stack details from a README: the rows of the tables whose first
 * header cell is empty (`| | |`).
 */
function stackRows(readme: ReadmeInfo): StackRow[] {
  const rows: StackRow[] = [];
  for (const table of readme.tables) {
    if (table.header[0] !== "") continue;
    for (const row of table.rows) {
      const label = row[0]?.replace(/\*\*/g, "");
      const value = row[1];
      if (label && value) rows.push({ label, html: inlineMarkdown(value) });
    }
  }
  return rows;
}

// -------------------------------------------------------------- performance

/**
 * The site shows Navree's own measurements only: of everything in `bench/`,
 * only Navree's results and its benchmark app are read, copied or shipped.
 */
const SUBJECT = "navree";

/** The machine, as `bench/results/method-notes.html` records it. */
const MACHINE = {
  cpu: "Intel Core Ultra 9 275HX",
  host: "Docker Desktop on WSL2",
  network: "one Docker bridge network",
};

/** The source the performance page shows: the code being measured. */
const CODE_FILES = ["src/deals/controller.rs", "src/deals/dto.rs"];

/** The app container's memory limit, from `startApp` in `bench/run.mjs`. */
const APP_MEM_GB = 4;

/** The five tests, as `bench/run.mjs` names them, with the request wrk sends. */
const TESTS: TestInfo[] = [
  {
    key: "json",
    label: "JSON",
    path: "GET /json",
    request: "GET /json",
    what: "Serialize a small object per request: routing and the HTTP pipeline, no database.",
    realistic: false,
  },
  {
    key: "db",
    label: "Single query",
    path: "GET /db",
    request: "GET /db",
    what: "Read one random row by primary key from the connection pool.",
    realistic: false,
  },
  {
    key: "queries",
    label: "20 queries",
    path: "GET /queries",
    request: "GET /queries?queries=20",
    what: "Twenty separate single-row reads, one after another.",
    realistic: false,
  },
  {
    key: "deals_list",
    label: "Auth + paginated list",
    path: "GET /api/deals",
    request: "GET /api/deals?page=1…50&per_page=20",
    what: "Verify the JWT, load the user, validate the query, count the deals and load a page of 20 with their owners.",
    realistic: true,
  },
  {
    key: "deals_create",
    label: "Auth + validated insert",
    path: "POST /api/deals",
    request: "POST /api/deals",
    what: "Verify the JWT, load the user, validate the body and insert the deal with INSERT … RETURNING.",
    realistic: true,
  },
];

type RawTest = {
  rps: number;
  requests: number;
  errors: number;
  latency_ms: { mean: number; p50: number; p75: number; p90: number; p99: number; p999: number; max: number };
  peak_mem_mb: number;
  avg_cpu_pct: number;
  pg_avg_cpu_pct: number;
  wrk_avg_cpu_pct: number;
  runs: { rps: number; errors: number; p99_ms: number }[];
};

type RawResult = {
  name: string;
  label: string;
  language: string;
  started_at: string;
  finished_at?: string;
  settings: {
    runs: number;
    duration_s: number;
    warmup_s: number;
    connections: number;
    threads: number;
    cpus: { ncpu: number; app: string; pg: string; load: string };
  };
  image_mb: number;
  startup_ms: number;
  idle_mem_mb: number;
  conformance: { ok: boolean; output: string };
  tests: Partial<Record<TestKey, RawTest>>;
};

function testResult(raw: RawTest): TestResult {
  return {
    rps: raw.rps,
    requests: raw.requests,
    errors: raw.errors,
    latencyMs: {
      mean: raw.latency_ms.mean,
      p50: raw.latency_ms.p50,
      p75: raw.latency_ms.p75,
      p90: raw.latency_ms.p90,
      p99: raw.latency_ms.p99,
      p999: raw.latency_ms.p999,
      max: raw.latency_ms.max,
    },
    peakMemMb: raw.peak_mem_mb,
    cpuPct: raw.avg_cpu_pct ?? 0,
    pgCpuPct: raw.pg_avg_cpu_pct ?? 0,
    wrkCpuPct: raw.wrk_avg_cpu_pct ?? 0,
    runs: (raw.runs ?? []).map((r) => ({ rps: r.rps, p99Ms: r.p99_ms, errors: r.errors })),
  };
}

function copyRaw(file: string, name: string, what: string, downloads: PerfData["downloads"]): string {
  const target = path.join(PUBLIC_DATA, "bench", name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(file, target);
  const href = `/data/bench/${name}`;
  downloads.push({ file: name, href, bytes: fs.statSync(file).size, what });
  return href;
}

function buildPerformance(): PerfData {
  // Start clean, so nothing but Navree's files is ever served.
  fs.rmSync(path.join(PUBLIC_DATA, "bench"), { recursive: true, force: true });
  fs.rmSync(path.join(GENERATED, "benchmarks.json"), { force: true });

  const file = path.join(RESULTS, `${SUBJECT}.json`);
  if (!fs.existsSync(file)) fail(`no ${SUBJECT}.json in ${RESULTS}`);
  const raw = readJson<RawResult>(file);

  const downloads: PerfData["downloads"] = [];
  const rawHref = copyRaw(file, `${SUBJECT}.json`, "The measured run: every test, all three runs", downloads);

  const results = {} as PerfData["results"];
  for (const test of TESTS) {
    const t = raw.tests?.[test.key];
    if (!t) fail(`${SUBJECT}.json has no result for ${test.key}`);
    results[test.key] = testResult(t);
  }

  const s = raw.settings;
  const conformance = /(\d+) checks passed, (\d+) failed/.exec(raw.conformance?.output ?? "");
  const sourceDir = `bench/frameworks/${SUBJECT}`;
  const env = path.join(RESULTS, "env.json");
  const docker = fs.existsSync(env) ? (readJson<{ docker?: string }>(env).docker ?? null) : null;

  // The same tests measured again at the end of the session (the drift check).
  let repeat: PerfData["repeat"] = null;
  const repeatFile = path.join(RESULTS, "extra", `${SUBJECT}-control.json`);
  if (fs.existsSync(repeatFile)) {
    const second = readJson<RawResult>(repeatFile);
    repeat = {
      startedAt: second.started_at,
      minutesLater: Math.round((Date.parse(second.started_at) - Date.parse(raw.started_at)) / 60000),
      rawHref: copyRaw(repeatFile, `${SUBJECT}-repeat.json`, "The same tests, measured again later in the session", downloads),
      tests: TESTS.flatMap((test) => {
        const a = raw.tests[test.key];
        const b = second.tests[test.key];
        if (!a || !b) return [];
        return [{ key: test.key, first: a.rps, second: b.rps, deltaPct: ((b.rps - a.rps) / a.rps) * 100 }];
      }),
    };
  }

  return {
    label: raw.label,
    language: raw.language,
    startedAt: raw.started_at,
    finishedAt: raw.finished_at ?? null,
    docker,
    machine: MACHINE,
    settings: {
      runs: s.runs,
      durationS: s.duration_s,
      warmupS: s.warmup_s,
      connections: s.connections,
      threads: s.threads,
      ncpu: s.cpus.ncpu,
      app: cpuSet(s.cpus.app),
      pg: cpuSet(s.cpus.pg),
      load: cpuSet(s.cpus.load),
      appMemGb: APP_MEM_GB,
    },
    tests: TESTS,
    results,
    startupMs: raw.startup_ms,
    idleMemMb: raw.idle_mem_mb,
    imageMb: raw.image_mb,
    peakMemMb: Math.max(...Object.values(results).map((r) => r.peakMemMb)),
    conformance: {
      ok: Boolean(raw.conformance?.ok),
      passed: conformance ? Number(conformance[1]) : null,
      failed: conformance ? Number(conformance[2]) : null,
    },
    stack: stackRows(parseReadme(path.join(REPO, sourceDir, "README.md"))),
    sourceDir,
    code: CODE_FILES.map((file) => {
      const full = path.join(REPO, sourceDir, file);
      if (!fs.existsSync(full)) fail(`${sourceDir}/${file} is missing`);
      return { file, source: fs.readFileSync(full, "utf8").replace(/\r\n/g, "\n") };
    }),
    rawHref,
    repeat,
    downloads,
  };
}

// --------------------------------------------------------------------- docs

type MdNode = {
  type: string;
  value?: string;
  depth?: number;
  name?: string | null;
  children?: MdNode[];
  attributes?: { type: string; name?: string; value?: unknown }[];
};

function nodeText(node: MdNode): string {
  if (node.type === "text" || node.type === "inlineCode") return node.value ?? "";
  if (node.type === "code") return node.value ?? "";
  if (node.type === "mdxjsEsm" || node.type === "mdxFlowExpression" || node.type === "mdxTextExpression") return "";
  // Block children (list items, table rows and cells) are separate words;
  // inline children (text next to `code`) are not.
  return (node.children ?? []).map(nodeText).join(BLOCK_PARENTS.has(node.type) ? " " : "");
}

const BLOCK_PARENTS = new Set(["list", "listItem", "table", "tableRow", "blockquote", "mdxJsxFlowElement"]);

function attribute(node: MdNode, name: string): string | null {
  const attr = node.attributes?.find((a) => a.type === "mdxJsxAttribute" && a.name === name);
  return typeof attr?.value === "string" ? attr.value : null;
}

/** The text of a page split into its introduction and its `##`/`###` sections. */
function chunks(content: string): { heading: string; id: string; text: string }[] {
  const processor = createProcessor({ remarkPlugins: [remarkGfm] });
  const tree = processor.parse(content) as unknown as MdNode;
  const slugger = new GithubSlugger();
  const out: { heading: string; id: string; text: string[] }[] = [{ heading: "", id: "", text: [] }];

  const walk = (node: MdNode) => {
    if (node.type === "heading") {
      // Slug the untrimmed text, exactly as rehype-slug does on the page.
      const raw = nodeText(node);
      const id = slugger.slug(raw);
      const text = raw.trim();
      if (node.depth === 2 || node.depth === 3) {
        out.push({ heading: text, id, text: [] });
        return;
      }
      out[out.length - 1].text.push(text);
      return;
    }
    if (node.type === "mdxJsxFlowElement" || node.type === "mdxJsxTextElement") {
      // The stub notice says nothing about the page itself.
      if (attribute(node, "title") === "This page is being written") return;
      const title = attribute(node, "title");
      if (title) out[out.length - 1].text.push(title);
      node.children?.forEach(walk);
      return;
    }
    if (node.type === "code") {
      out[out.length - 1].text.push((node.value ?? "").slice(0, 1200));
      return;
    }
    if (["paragraph", "list", "table", "blockquote"].includes(node.type)) {
      out[out.length - 1].text.push(nodeText(node));
      return;
    }
    node.children?.forEach(walk);
  };
  tree.children?.forEach(walk);

  return out
    .map((c) => ({ heading: c.heading, id: c.id, text: c.text.join(" ").replace(/\s+/g, " ").trim().slice(0, 4000) }))
    .filter((c, i) => i > 0 || c.text);
}

function buildDocs(perf: PerfData): SearchDoc[] {
  const docs: SearchDoc[] = [];
  const problems: string[] = [];
  const listed = new Set<string>();

  for (const section of sections) {
    for (const page of section.pages) {
      const rel = `${section.slug}/${page}.mdx`;
      listed.add(rel);
      const file = path.join(DOCS, rel);
      if (!fs.existsSync(file)) {
        problems.push(`content/docs/${rel} is in the manifest but does not exist`);
        continue;
      }
      const { data, content } = matter(fs.readFileSync(file, "utf8"));
      if (typeof data.title !== "string" || !data.title.trim()) problems.push(`content/docs/${rel} has no title`);
      if (typeof data.description !== "string" || !data.description.trim()) {
        problems.push(`content/docs/${rel} has no description`);
      }
      const href = `/docs/${section.slug}/${page}/`;
      const title = String(data.title ?? page);
      let parts: ReturnType<typeof chunks> = [];
      try {
        parts = chunks(content);
      } catch (error) {
        problems.push(`content/docs/${rel} is not valid MDX: ${(error as Error).message}`);
      }
      if (!parts.length || parts[0].heading) parts.unshift({ heading: "", id: "", text: "" });
      parts[0].text = [String(data.description ?? ""), parts[0].text].join(" ").trim();
      for (const part of parts) {
        docs.push({
          id: part.id ? `${href}#${part.id}` : href,
          href: part.id ? `${href}#${part.id}` : href,
          page: title,
          section: section.title,
          heading: part.heading,
          text: part.text.replace(/`/g, ""),
        });
      }
    }
  }

  // Files that exist but aren't in the manifest are not published.
  for (const dir of fs.readdirSync(DOCS, { withFileTypes: true })) {
    if (!dir.isDirectory()) continue;
    for (const file of fs.readdirSync(path.join(DOCS, dir.name))) {
      const rel = `${dir.name}/${file}`;
      if (file.endsWith(".mdx") && !listed.has(rel)) {
        console.warn(`prebuild: content/docs/${rel} is not in content/docs/manifest.ts, so it is not published`);
      }
    }
  }

  if (problems.length) fail(`the docs have problems:\n  - ${problems.join("\n  - ")}`);

  // The performance page is in the sidebar, so it is searchable too.
  const performancePage = links.find((l) => l.href.startsWith("/performance"));
  if (performancePage) {
    const fmt = (n: number) => Math.round(n).toLocaleString("en-US");
    const r = perf.results;
    const sectionsOfPerformance: [string, string, string][] = [
      [
        "",
        "",
        `Navree's own measurements on a CRM-style API: ${fmt(r.deals_list.rps)} authenticated paginated lists and ${fmt(r.deals_create.rps)} validated inserts a second on ${perf.settings.app.count} CPUs, ${fmt(perf.peakMemMb)} MB peak memory, ready in ${fmt(perf.startupMs)} ms.`,
      ],
      ["results", "Results", "Requests per second, p50 and p99 latency, peak memory and CPU for each test: JSON, single query, 20 queries, auth + paginated list, auth + validated insert."],
      ["workload", "The workload", "The benchmark API: JWT authentication, validation, a paginated list with owners, inserts, single and multiple queries, plain JSON. The request pipeline every request goes through."],
      ["method", "The machine and the method", "Docker containers with fixed CPUs for the app, PostgreSQL and wrk; warm-up and measured runs; wrk on the monotonic clock; the conformance test; the repeat run."],
      ["efficiency", "What makes it efficient", "One binary, Tokio and Hyper, pooled PostgreSQL connections, no garbage collector."],
      ["next", "What we are optimizing next", "Per-request overhead in Navree: pool pings, set_config round trips, a task per request, locks, metrics labels, the user lookup, JSON parsing, change events."],
      ["reproduce", "Reproduce it", "The commands to run the benchmark yourself with Docker and Node.js, and the raw result files."],
    ];
    for (const [id, heading, text] of sectionsOfPerformance) {
      const href = id ? `${performancePage.href}#${id}` : performancePage.href;
      docs.push({ id: href, href, page: performancePage.title, section: "Performance", heading, text });
    }
  }
  return docs;
}

// ------------------------------------------------------------------- crates

export type CrateInfo = { name: string; layer: number; module: string | null; description: string };

/**
 * Every framework crate with its layer and description, for "everything in
 * the box": layers from the HIERARCHY table that
 * `crates/navree/tests/dependency_graph.rs` enforces, descriptions from each
 * crate's Cargo.toml.
 */
function buildCrates(): CrateInfo[] {
  const graph = fs.readFileSync(path.join(REPO, "crates/navree/tests/dependency_graph.rs"), "utf8");
  const crates: CrateInfo[] = [];
  // Names may hold digits (navree-i18n), so every crate of the table is counted.
  for (const match of graph.matchAll(/rule\("(navree(?:-[a-z0-9-]+)?)",\s*(\d+)/g)) {
    const [, name, layer] = match;
    const manifest = path.join(REPO, "crates", name, "Cargo.toml");
    if (!fs.existsSync(manifest)) continue;
    const raw = /^description\s*=\s*"(.*)"\s*$/m.exec(fs.readFileSync(manifest, "utf8"))?.[1] ?? "";
    // "Navree QueueModule: database and memory job queues, ..." → module + text.
    const prefixed = /^Navree (\w+Module): (.*)$/.exec(raw);
    const text = prefixed ? prefixed[2] : raw.replace(/^Navree( kernel| contracts| attribute macros| testing kit)?: /i, "");
    crates.push({
      name,
      layer: Number(layer),
      module: prefixed ? prefixed[1] : null,
      description: text.charAt(0).toUpperCase() + text.slice(1),
    });
  }
  if (!crates.length) fail("no crates found in crates/navree/tests/dependency_graph.rs");
  return crates;
}

// --------------------------------------------------------------------- main

const started = performance.now();
const perf = buildPerformance();
writeFile(path.join(GENERATED, "performance.json"), JSON.stringify(perf, null, 2));
const crates = buildCrates();
writeFile(path.join(GENERATED, "crates.json"), JSON.stringify(crates, null, 2));

const docs = buildDocs(perf);
const index = new MiniSearch<SearchDoc>(searchOptions);
index.addAll(docs);
writeFile(path.join(PUBLIC_DATA, "search-index.json"), JSON.stringify(index));

console.log(
  `prebuild: performance data, ${perf.downloads.length} raw files, ${crates.length} crates, ` +
    `${docs.length} search entries (${Math.round(performance.now() - started)} ms)`,
);
