/**
 * Runs before every `dev`, `build` and `typecheck` (Node strips the types;
 * no build step). It generates everything the site reads at build time:
 *
 * - `src/generated/benchmarks.json`: the benchmark results, from
 *   `bench/results/*.json` (not `env.json`), `bench/results/extra/` (the
 *   drift check and the Laravel-FPM run with default kernel settings),
 *   `bench/frameworks.json` and each implementation's README table.
 * - `public/data/bench/`: the raw result files, for download.
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
import type { BenchData, CpuSet, Framework, StackRow, TestInfo, TestKey, TestResult } from "../src/lib/bench-types.ts";
import { searchOptions, type SearchDoc } from "../src/lib/search-options.ts";

const WEBSITE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const REPO = path.resolve(WEBSITE, "../..");
const BENCH = path.join(REPO, "bench");
const RESULTS = path.join(BENCH, "results");
const DOCS = path.join(WEBSITE, "content/docs");
const GENERATED = path.join(WEBSITE, "src/generated");
const PUBLIC_DATA = path.join(WEBSITE, "public/data");

/** The framework the site is about; it gets the accent in every chart. */
const HIGHLIGHT = "navis";

/** The five tests, as `bench/run.mjs` and `bench/report.mjs` name them. */
const TESTS: TestInfo[] = [
  { key: "json", label: "JSON", path: "GET /json", what: "Serialize a small object: routing and HTTP overhead only." },
  { key: "db", label: "Single query", path: "GET /db", what: "One random row by primary key." },
  { key: "queries", label: "20 queries", path: "GET /queries?queries=20", what: "20 separate random-row queries per request." },
  {
    key: "deals_list",
    label: "Auth + paginated list",
    path: "GET /api/deals",
    what: "JWT check, user lookup, a count and one page of 20 deals with their owners.",
  },
  {
    key: "deals_create",
    label: "Auth + validated insert",
    path: "POST /api/deals",
    what: "JWT check, user lookup, body validation and an INSERT … RETURNING.",
  },
];

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
 * The stack details of `name` from its README: the rows of the tables whose
 * first header cell is empty (`| | |`). A table with one column per variant
 * (Laravel: Octane and FPM) contributes the column whose header names it.
 */
function stackRows(readme: ReadmeInfo, name: string): StackRow[] {
  const rows: StackRow[] = [];
  for (const table of readme.tables) {
    if (table.header[0] !== "") continue;
    let column = 1;
    if (table.header.length > 2) {
      const found = table.header.findIndex((h, i) => i > 0 && h.includes(name));
      if (found === -1) continue;
      column = found;
    }
    for (const row of table.rows) {
      const label = row[0]?.replace(/\*\*/g, "");
      const value = row[column];
      if (label && value) rows.push({ label, html: inlineMarkdown(value) });
    }
  }
  return rows;
}

// --------------------------------------------------------------- benchmarks

type RawTest = {
  rps: number;
  requests: number;
  errors: number;
  latency_ms: { mean: number; p50: number; p90: number; p99: number; p999: number; max: number };
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
  category: "full-stack" | "micro";
  started_at: string;
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

type FrameworkEntry = {
  name: string;
  label: string;
  language: string;
  category: "full-stack" | "micro";
  context?: string;
  dockerfile?: string;
  note?: string;
};

function testResult(raw: RawTest): TestResult {
  return {
    rps: raw.rps,
    requests: raw.requests,
    errors: raw.errors,
    latencyMs: {
      mean: raw.latency_ms.mean,
      p50: raw.latency_ms.p50,
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

/** The implementation's directory, relative to the repository. */
function sourceDir(entry: FrameworkEntry): string {
  if (entry.dockerfile) return path.posix.join("bench", path.posix.dirname(entry.dockerfile));
  if (entry.context && entry.context !== "..") return path.posix.join("bench", entry.context);
  return path.posix.join("bench/frameworks", entry.name);
}

function copyRaw(file: string, relative: string, downloads: BenchData["downloads"]): string {
  const target = path.join(PUBLIC_DATA, "bench", relative);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(file, target);
  const href = `/data/bench/${relative.split(path.sep).join("/")}`;
  downloads.push({ file: relative.split(path.sep).join("/"), href, bytes: fs.statSync(file).size });
  return href;
}

function buildBenchmarks(): BenchData {
  const entries = readJson<FrameworkEntry[]>(path.join(BENCH, "frameworks.json"));
  fs.rmSync(path.join(PUBLIC_DATA, "bench"), { recursive: true, force: true });

  const downloads: BenchData["downloads"] = [];
  const frameworks: Framework[] = [];
  const missing: BenchData["missing"] = [];
  let settings: BenchData["settings"] | null = null;

  for (const entry of entries) {
    const file = path.join(RESULTS, `${entry.name}.json`);
    if (!fs.existsSync(file)) {
      missing.push({ name: entry.name, label: entry.label });
      continue;
    }
    const raw = readJson<RawResult>(file);
    const rawHref = copyRaw(file, `${entry.name}.json`, downloads);
    const results: Framework["results"] = {};
    for (const test of TESTS) {
      const t = raw.tests?.[test.key];
      if (t) results[test.key] = testResult(t);
    }
    const peaks = Object.values(results).map((r) => r.peakMemMb);
    const conformance = /(\d+) checks passed, (\d+) failed/.exec(raw.conformance?.output ?? "");
    const dir = sourceDir(entry);
    const readme = parseReadme(path.join(REPO, dir, "README.md"));
    frameworks.push({
      name: entry.name,
      label: entry.label,
      language: entry.language,
      category: entry.category,
      note: entry.note ?? null,
      startupMs: raw.startup_ms,
      idleMemMb: raw.idle_mem_mb,
      imageMb: raw.image_mb,
      peakMemMb: peaks.length ? Math.max(...peaks) : 0,
      conformance: {
        ok: Boolean(raw.conformance?.ok),
        passed: conformance ? Number(conformance[1]) : null,
        failed: conformance ? Number(conformance[2]) : null,
        output: raw.conformance?.output ?? "",
      },
      results,
      startedAt: raw.started_at,
      stack: stackRows(readme, entry.name),
      sourceDir: dir,
      readmeTitle: readme.title || entry.label,
      rawHref,
    });
    if (entry.name === HIGHLIGHT || !settings) {
      const s = raw.settings;
      settings = {
        runs: s.runs,
        durationS: s.duration_s,
        warmupS: s.warmup_s,
        connections: s.connections,
        threads: s.threads,
        ncpu: s.cpus.ncpu,
        app: cpuSet(s.cpus.app),
        pg: cpuSet(s.cpus.pg),
        load: cpuSet(s.cpus.load),
      };
    }
  }

  if (!settings) fail(`no benchmark results in ${RESULTS}`);
  if (!frameworks.some((f) => f.name === HIGHLIGHT)) fail(`no ${HIGHLIGHT}.json in ${RESULTS}`);

  // The drift check: the highlighted framework measured first and last.
  const extra = path.join(RESULTS, "extra");
  let drift: BenchData["drift"] = null;
  const firstFile = path.join(extra, `${HIGHLIGHT}-run1.json`);
  const secondFile = path.join(extra, `${HIGHLIGHT}-control.json`);
  if (fs.existsSync(firstFile) && fs.existsSync(secondFile)) {
    const first = readJson<RawResult>(firstFile);
    const second = readJson<RawResult>(secondFile);
    copyRaw(firstFile, path.join("extra", path.basename(firstFile)), downloads);
    copyRaw(secondFile, path.join("extra", path.basename(secondFile)), downloads);
    drift = {
      firstAt: first.started_at,
      secondAt: second.started_at,
      tests: TESTS.flatMap((test) => {
        const a = first.tests[test.key];
        const b = second.tests[test.key];
        if (!a || !b) return [];
        return [{ key: test.key, first: a.rps, second: b.rps, deltaPct: ((b.rps - a.rps) / a.rps) * 100 }];
      }),
    };
  }

  // Laravel on PHP-FPM with Docker's default sysctls (before tcp_tw_reuse).
  let fpmDefault: BenchData["fpmDefault"] = null;
  const fpmFile = path.join(extra, "laravel-fpm-default-sysctls.json");
  if (fs.existsSync(fpmFile)) {
    const fpm = readJson<RawResult>(fpmFile);
    fpmDefault = {
      label: fpm.label,
      rawHref: copyRaw(fpmFile, path.join("extra", path.basename(fpmFile)), downloads),
      tests: TESTS.flatMap((test) => {
        const t = fpm.tests[test.key];
        if (!t) return [];
        const total = t.requests || 1;
        return [{ key: test.key, rps: t.rps, requests: t.requests, errors: t.errors, errorPct: (t.errors / total) * 100 }];
      }),
    };
  }

  return { highlight: HIGHLIGHT, settings, tests: TESTS, frameworks, missing, drift, fpmDefault, downloads };
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
  return (node.children ?? []).map(nodeText).join(node.type === "list" || node.type === "table" ? " " : "");
}

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

function buildDocs(bench: BenchData): SearchDoc[] {
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

  // The benchmarks page is in the sidebar, so it is searchable too.
  const benchmarks = links.find((l) => l.href.startsWith("/benchmarks"));
  if (benchmarks) {
    const sectionsOfBenchmarks: [string, string, string][] = [
      ["", "", `How Navis compares with ${bench.frameworks.length - 1} other web frameworks on the same API and machine: throughput, latency, memory and startup.`],
      ["results", "Results", "Requests per second for every framework and test, ranked. JSON, single query, 20 queries, auth + paginated list, auth + validated insert."],
      ["latency", "Latency", "p99 latency per test for every framework."],
      ["memory-and-startup", "Memory and startup", "Peak memory under load, idle memory, image size and startup time."],
      ["all-results", "All results", "The full results table: requests per second and p99 latency for every test."],
      ["stacks", "The stacks", "Versions, server and workers, database library and pool, JWT and validation of each implementation."],
      ["methodology", "Methodology", "The API contract, the conformance gate, fairness rules, CPU pinning, warmup and runs, the drift check."],
      ["caveats", "Caveats", "Load-generator limits, database-bound tests, Laravel PHP-FPM port exhaustion, wrk clock fix."],
      ["where-navis-is-slower", "Where Navis is slower", "Where Navis loses time per request and why: pool pings, set_config, task spawning, locks."],
      ["reproduce", "Reproduce it yourself", "Commands to run the benchmark, the source of each implementation and the raw JSON."],
    ];
    for (const [id, heading, text] of sectionsOfBenchmarks) {
      const href = id ? `${benchmarks.href}#${id}` : benchmarks.href;
      docs.push({ id: href, href, page: benchmarks.title, section: "Benchmarks", heading, text });
    }
  }
  return docs;
}

// ------------------------------------------------------------------- crates

export type CrateInfo = { name: string; layer: number; module: string | null; description: string };

/**
 * Every framework crate with its layer and description, for "everything in
 * the box": layers from the HIERARCHY table that
 * `crates/navis/tests/dependency_graph.rs` enforces, descriptions from each
 * crate's Cargo.toml.
 */
function buildCrates(): CrateInfo[] {
  const graph = fs.readFileSync(path.join(REPO, "crates/navis/tests/dependency_graph.rs"), "utf8");
  const crates: CrateInfo[] = [];
  for (const match of graph.matchAll(/rule\("(navis(?:-[a-z-]+)?)",\s*(\d+)/g)) {
    const [, name, layer] = match;
    const manifest = path.join(REPO, "crates", name, "Cargo.toml");
    if (!fs.existsSync(manifest)) continue;
    const raw = /^description\s*=\s*"(.*)"\s*$/m.exec(fs.readFileSync(manifest, "utf8"))?.[1] ?? "";
    // "Navis QueueModule: database and memory job queues, ..." → module + text.
    const prefixed = /^Navis (\w+Module): (.*)$/.exec(raw);
    const text = prefixed ? prefixed[2] : raw.replace(/^Navis( kernel| contracts| attribute macros| testing kit)?: /i, "");
    crates.push({
      name,
      layer: Number(layer),
      module: prefixed ? prefixed[1] : null,
      description: text.charAt(0).toUpperCase() + text.slice(1),
    });
  }
  if (!crates.length) fail("no crates found in crates/navis/tests/dependency_graph.rs");
  return crates;
}

// --------------------------------------------------------------------- main

const started = performance.now();
const bench = buildBenchmarks();
writeFile(path.join(GENERATED, "benchmarks.json"), JSON.stringify(bench, null, 2));
const crates = buildCrates();
writeFile(path.join(GENERATED, "crates.json"), JSON.stringify(crates, null, 2));

const docs = buildDocs(bench);
const index = new MiniSearch<SearchDoc>(searchOptions);
index.addAll(docs);
writeFile(path.join(PUBLIC_DATA, "search-index.json"), JSON.stringify(index));

console.log(
  `prebuild: ${bench.frameworks.length} benchmark results, ${bench.downloads.length} raw files, ${crates.length} crates, ` +
    `${docs.length} search entries (${Math.round(performance.now() - started)} ms)`,
);
