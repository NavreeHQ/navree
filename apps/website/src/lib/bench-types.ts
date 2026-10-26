/**
 * The shape of `src/generated/benchmarks.json`, which `scripts/prebuild.ts`
 * writes from `bench/results/*.json`, `bench/frameworks.json` and each
 * implementation's README. Every number on the site comes from there.
 */

export type TestKey = "json" | "db" | "queries" | "deals_list" | "deals_create";

export type TestInfo = {
  key: TestKey;
  label: string;
  /** The request wrk sends. */
  path: string;
  /** What one request does. */
  what: string;
};

export type TestResult = {
  /** Requests per second of the median run. */
  rps: number;
  requests: number;
  errors: number;
  latencyMs: { mean: number; p50: number; p90: number; p99: number; p999: number; max: number };
  peakMemMb: number;
  /** Average CPU while the test ran, in percent of one core (800 = all 8). */
  cpuPct: number;
  pgCpuPct: number;
  wrkCpuPct: number;
  /** The three measured runs. */
  runs: { rps: number; p99Ms: number; errors: number }[];
};

export type StackRow = {
  label: string;
  /** The README cell as safe HTML (inline code, bold and links only). */
  html: string;
};

export type Framework = {
  name: string;
  label: string;
  language: string;
  category: "full-stack" | "micro";
  note: string | null;
  startupMs: number;
  idleMemMb: number;
  imageMb: number;
  /** The highest peak memory across all tests. */
  peakMemMb: number;
  conformance: { ok: boolean; passed: number | null; failed: number | null; output: string };
  results: Partial<Record<TestKey, TestResult>>;
  startedAt: string;
  stack: StackRow[];
  /** The implementation's directory, e.g. `bench/frameworks/navree`. */
  sourceDir: string;
  readmeTitle: string;
  /** The raw result file, as served by the site. */
  rawHref: string;
};

export type CpuSet = { range: string; count: number };

export type BenchData = {
  highlight: string;
  settings: {
    runs: number;
    durationS: number;
    warmupS: number;
    connections: number;
    threads: number;
    ncpu: number;
    app: CpuSet;
    pg: CpuSet;
    load: CpuSet;
  };
  tests: TestInfo[];
  frameworks: Framework[];
  /** Listed in `bench/frameworks.json` but without a result file. */
  missing: { name: string; label: string }[];
  /** The highlighted framework measured again after every other run. */
  drift: {
    firstAt: string;
    secondAt: string;
    tests: { key: TestKey; first: number; second: number; deltaPct: number }[];
  } | null;
  /** Laravel on PHP-FPM with Docker's default kernel settings. */
  fpmDefault: {
    label: string;
    rawHref: string;
    tests: { key: TestKey; rps: number; requests: number; errors: number; errorPct: number }[];
  } | null;
  downloads: { file: string; href: string; bytes: number }[];
};
