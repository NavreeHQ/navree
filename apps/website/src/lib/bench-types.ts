/**
 * The shape of `src/generated/performance.json`, which `scripts/prebuild.ts`
 * writes from Navree's own result files in `bench/results/` and the README
 * of its benchmark app (`bench/frameworks/navree`). Every performance number
 * on the site comes from there. Only Navree's results are read: the site
 * describes Navree's own measurements.
 */

export type TestKey = "json" | "db" | "queries" | "deals_list" | "deals_create";

export type TestInfo = {
  key: TestKey;
  label: string;
  /** The route, e.g. `GET /api/deals`. */
  path: string;
  /** The exact request the load generator sends. */
  request: string;
  /** What one request does, in a sentence. */
  what: string;
  /** The authenticated endpoints that do what a CRUD API does. */
  realistic: boolean;
};

export type TestResult = {
  /** Requests per second of the median run. */
  rps: number;
  /** Requests completed in the median run. */
  requests: number;
  errors: number;
  latencyMs: { mean: number; p50: number; p75: number; p90: number; p99: number; p999: number; max: number };
  peakMemMb: number;
  /** Average CPU of the app while the test ran, in percent of one CPU (800 = all 8). */
  cpuPct: number;
  /** The same for PostgreSQL and for the load generator. */
  pgCpuPct: number;
  wrkCpuPct: number;
  /** The three measured runs, in order. */
  runs: { rps: number; p99Ms: number; errors: number }[];
};

export type StackRow = {
  label: string;
  /** The README cell as safe HTML (inline code, bold and links only). */
  html: string;
};

export type CpuSet = { range: string; count: number };

export type PerfData = {
  label: string;
  language: string;
  /** When the measured run started and finished (ISO 8601). */
  startedAt: string;
  finishedAt: string | null;
  /** Docker's version on the machine. */
  docker: string | null;
  /** The machine, as `bench/results/method-notes.html` records it. */
  machine: { cpu: string; host: string; network: string };
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
    /** Memory limit of the app container. */
    appMemGb: number;
  };
  tests: TestInfo[];
  results: Record<TestKey, TestResult>;
  /** Container start to the first 200 from `GET /json`. */
  startupMs: number;
  idleMemMb: number;
  imageMb: number;
  /** The highest peak memory across all tests. */
  peakMemMb: number;
  conformance: { ok: boolean; passed: number | null; failed: number | null };
  /** Navree's build of the benchmark app, from its README. */
  stack: StackRow[];
  /** The app's directory, `bench/frameworks/navree`. */
  sourceDir: string;
  /** Source files of the app, relative to `sourceDir`: the code being measured. */
  code: { file: string; source: string }[];
  /** The raw result file, as the site serves it. */
  rawHref: string;
  /** The same tests measured again later in the session, to check for drift. */
  repeat: {
    startedAt: string;
    minutesLater: number;
    rawHref: string;
    tests: { key: TestKey; first: number; second: number; deltaPct: number }[];
  } | null;
  downloads: { file: string; href: string; bytes: number; what: string }[];
};
