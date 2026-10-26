#!/usr/bin/env node
// Runs the benchmark one framework at a time (SPEC.md §6).
//
//   node bench/run.mjs                       every framework in frameworks.json
//   node bench/run.mjs --only navis,axum      just these
//   node bench/run.mjs --smoke --only navis   build + conformance + 5 s per test (for implementers)
//   node bench/run.mjs --skip-build          reuse built images
//   node bench/run.mjs --skip-done           skip frameworks that already have results
//   node bench/run.mjs --report              only rewrite results/summary.md
//
// Options: --runs 3 --duration 15 --warmup 10 --connections 256 --threads 8
//          --app-cpus 0-7 --pg-cpus 8-17 --load-cpus 18-23 --db <name>
import { execFile, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { JWT_ISSUER, JWT_SECRET, TOKEN } from "./tools/fixtures.mjs";

const execFileAsync = promisify(execFile);
const BENCH = dirname(fileURLToPath(import.meta.url));
const FRAMEWORKS = JSON.parse(readFileSync(join(BENCH, "frameworks.json"), "utf8"));

export const TESTS = [
  { key: "json", label: "JSON", path: "/json", script: "report.lua" },
  { key: "db", label: "Single query", path: "/db", script: "report.lua" },
  { key: "queries", label: "20 queries", path: "/queries?queries=20", script: "report.lua" },
  { key: "deals_list", label: "Auth + paginated list", path: "/api/deals", script: "deals-list.lua", auth: true },
  { key: "deals_create", label: "Auth + validated insert", path: "/api/deals", script: "deals-create.lua", auth: true },
];

// ---------------------------------------------------------------- options

function parseArgs(argv) {
  const opts = {
    only: null,
    smoke: false,
    skipBuild: false,
    skipDone: false,
    report: false,
    runs: 3,
    duration: 15,
    warmup: 10,
    connections: 256,
    threads: 6,
    appCpus: null,
    pgCpus: null,
    loadCpus: null,
    db: null,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const next = () => argv[++i];
    switch (arg) {
      case "--only": opts.only = next().split(",").map((s) => s.trim()).filter(Boolean); break;
      case "--smoke": opts.smoke = true; break;
      case "--skip-build": opts.skipBuild = true; break;
      case "--skip-done": opts.skipDone = true; break;
      case "--report": opts.report = true; break;
      case "--runs": opts.runs = Number(next()); break;
      case "--duration": opts.duration = Number(next()); break;
      case "--warmup": opts.warmup = Number(next()); break;
      case "--connections": opts.connections = Number(next()); break;
      case "--threads": opts.threads = Number(next()); break;
      case "--app-cpus": opts.appCpus = next(); break;
      case "--pg-cpus": opts.pgCpus = next(); break;
      case "--load-cpus": opts.loadCpus = next(); break;
      case "--db": opts.db = next(); break;
      default: throw new Error(`unknown option ${arg}`);
    }
  }
  if (opts.smoke) {
    opts.runs = 1;
    opts.duration = 5;
    opts.warmup = 0;
    opts.connections = 64;
    opts.threads = 2;
  }
  return opts;
}

// ---------------------------------------------------------------- docker helpers

function docker(args, { allowFail = false, input, stdio } = {}) {
  const r = spawnSync("docker", args, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024, input, stdio });
  if (r.error) throw new Error(`docker ${args[0]}: ${r.error.message}`);
  if (r.status !== 0 && !allowFail) {
    throw new Error(`docker ${args.join(" ")} failed (${r.status}):\n${(r.stderr || r.stdout || "").slice(-4000)}`);
  }
  return r;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (msg) => console.log(`[${new Date().toISOString().slice(11, 19)}] ${msg}`);

function cpuLayout(opts) {
  // 24 CPUs: the app 8, PostgreSQL 10 (so the database is rarely the
  // limit), wrk 6. Smaller machines get the same proportions.
  const ncpu = Number(docker(["info", "--format", "{{.NCPU}}"]).stdout.trim());
  const range = (from, count) => `${from}-${from + count - 1}`;
  const app = Math.max(1, Math.min(8, Math.floor((ncpu * 8) / 24)));
  const pg = Math.max(1, Math.min(10, Math.floor((ncpu * 10) / 24)));
  const load = Math.max(1, Math.min(6, ncpu - app - pg));
  return {
    ncpu,
    app: opts.appCpus ?? range(0, app),
    pg: opts.pgCpus ?? range(app, pg),
    load: opts.loadCpus ?? range(app + pg, load),
  };
}

function ensureNetwork() {
  if (docker(["network", "inspect", "bench"], { allowFail: true }).status !== 0) {
    docker(["network", "create", "bench"]);
  }
}

async function ensurePostgres(cpus) {
  const running = docker(["inspect", "-f", "{{.State.Running}}", "bench-pg"], { allowFail: true });
  if (running.status !== 0 || running.stdout.trim() !== "true") {
    log("starting PostgreSQL (bench-pg)");
    docker(["rm", "-f", "bench-pg"], { allowFail: true });
    docker([
      "run", "-d", "--name", "bench-pg", "--network", "bench",
      "--cpuset-cpus", cpus.pg, "--memory", "6g", "--shm-size", "1g",
      "--tmpfs", "/var/lib/postgresql/data:rw,size=4g",
      "-e", "POSTGRES_USER=bench", "-e", "POSTGRES_PASSWORD=bench", "-e", "POSTGRES_DB=bench_template",
      "-v", `${join(BENCH, "db")}:/docker-entrypoint-initdb.d:ro`,
      "-p", "127.0.0.1:55432:5432",
      "postgres:17-bookworm",
      "-c", "max_connections=400", "-c", "shared_buffers=1GB", "-c", "effective_cache_size=3GB",
      "-c", "work_mem=16MB", "-c", "synchronous_commit=off", "-c", "max_wal_size=1GB",
      "-c", "checkpoint_timeout=10min", "-c", "random_page_cost=1.1", "-c", "effective_io_concurrency=200",
      "-c", "log_min_messages=warning",
    ]);
  }
  for (let i = 0; i < 120; i++) {
    const ready = docker(["exec", "bench-pg", "pg_isready", "-h", "127.0.0.1", "-U", "bench", "-d", "bench_template"], { allowFail: true });
    if (ready.status === 0) {
      const exists = psql("postgres", "select count(*) from pg_database where datname = 'bench_template'", true);
      if (exists === "1") return;
    }
    await sleep(1000);
  }
  throw new Error("PostgreSQL did not become ready:\n" + docker(["logs", "--tail", "50", "bench-pg"], { allowFail: true }).stderr);
}

function psql(db, sql, allowFail = false) {
  const r = docker(["exec", "bench-pg", "psql", "-h", "127.0.0.1", "-U", "bench", "-d", db, "-v", "ON_ERROR_STOP=1", "-tAc", sql], { allowFail });
  return r.status === 0 ? r.stdout.trim() : null;
}

async function resetDatabase(db) {
  // Retried: two runs copying the template at the same moment can collide.
  for (let attempt = 1; ; attempt++) {
    try {
      psql("postgres", `DROP DATABASE IF EXISTS ${db} WITH (FORCE)`);
      psql("postgres", `CREATE DATABASE ${db} TEMPLATE bench_template`);
      break;
    } catch (error) {
      if (attempt >= 10) throw error;
      await sleep(1000 + Math.random() * 2000);
    }
  }
  psql("postgres", "CHECKPOINT");
}

function ensureWrkImage() {
  log("building bench-wrk");
  docker(["build", "-q", "-t", "bench-wrk", join(BENCH, "wrk")]);
}

function buildImage(fw, opts) {
  const image = `bench-${fw.name}`;
  if (opts.skipBuild && docker(["image", "inspect", image], { allowFail: true }).status === 0) return;
  const context = resolve(BENCH, fw.context ?? `frameworks/${fw.name}`);
  const dockerfile = resolve(BENCH, fw.dockerfile ?? `frameworks/${fw.name}/Dockerfile`);
  log(`building ${image}`);
  const started = Date.now();
  const r = spawnSync("docker", ["build", "-t", image, "-f", dockerfile, context], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  writeFileSync(join(logDir(opts), `${fw.name}.build.log`), `${r.stdout ?? ""}\n${r.stderr ?? ""}`);
  if (r.status !== 0) {
    throw new Error(`build failed, see results/logs/${fw.name}.build.log:\n${(r.stderr ?? "").slice(-3000)}`);
  }
  log(`built ${image} in ${((Date.now() - started) / 1000).toFixed(0)} s`);
}

function memMb(text) {
  const m = /([\d.]+)\s*([KMG]?i?B)/i.exec(text);
  if (!m) return null;
  const unit = m[2].toLowerCase();
  const factor = unit.startsWith("g") ? 1024 : unit.startsWith("m") ? 1 : unit.startsWith("k") ? 1 / 1024 : 1 / 1048576;
  return Number(m[1]) * factor;
}

/** Memory and CPU of each container, keyed by name. */
async function statsAll(containers) {
  try {
    const { stdout } = await execFileAsync("docker", ["stats", "--no-stream", "--format", "{{.Name}}|{{.MemUsage}}|{{.CPUPerc}}", ...containers]);
    const out = {};
    for (const line of stdout.trim().split(/\r?\n/)) {
      const [name, mem, cpu] = line.trim().split("|");
      out[name] = { memMb: memMb(mem), cpu: Number.parseFloat(cpu) };
    }
    return out;
  } catch {
    return {};
  }
}

async function stats(container) {
  return (await statsAll([container]))[container] ?? null;
}

function startApp(fw, db, cpus) {
  const name = `bench-app-${fw.name}`;
  docker(["rm", "-f", name], { allowFail: true });
  const env = {
    DATABASE_URL: `postgres://bench:bench@bench-pg:5432/${db}`,
    DB_HOST: "bench-pg",
    DB_PORT: "5432",
    DB_NAME: db,
    DB_USER: "bench",
    DB_PASSWORD: "bench",
    JWT_SECRET,
    JWT_ISSUER,
  };
  docker([
    "run", "-d", "--name", name, "--network", "bench",
    "--cpuset-cpus", cpus.app, "--memory", "4g", "--memory-swap", "4g",
    // Per-framework container options from frameworks.json (run_args).
    ...(fw.run_args ?? []),
    "-p", "127.0.0.1::8080",
    ...Object.entries(env).flatMap(([k, v]) => ["-e", `${k}=${v}`]),
    `bench-${fw.name}`,
  ]);
  const port = docker(["port", name, "8080/tcp"]).stdout.trim().split("\n")[0].split(":").pop();
  return { name, url: `http://127.0.0.1:${port}` };
}

async function waitReady(app, timeoutMs = 90_000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    try {
      const res = await fetch(`${app.url}/json`, { signal: AbortSignal.timeout(2000) });
      if (res.status === 200) return Date.now() - started;
    } catch {
      // not listening yet
    }
    const state = docker(["inspect", "-f", "{{.State.Running}}", app.name], { allowFail: true }).stdout.trim();
    if (state !== "true") throw new Error("the container exited during startup");
    await sleep(100);
  }
  throw new Error(`not ready after ${timeoutMs / 1000} s`);
}

function runConformance(app) {
  const r = spawnSync(process.execPath, [join(BENCH, "tools", "conformance.mjs"), app.url], { encoding: "utf8" });
  return { ok: r.status === 0, output: `${r.stdout}${r.stderr}`.trim() };
}

/**
 * One long-lived load-generator container per app: every wrk run is a
 * `docker exec` in it. A fresh container per run sometimes cannot connect
 * for its first seconds, and wrk counts not-yet-connected sockets as
 * timeouts. Primed with a short unmeasured run.
 */
function startLoadGenerator(app, cpus) {
  const name = `bench-wrk-${app.name.replace(/^bench-app-/, "")}`;
  docker(["rm", "-f", name], { allowFail: true });
  docker(["run", "-d", "--name", name, "--network", "bench", "--cpuset-cpus", cpus.load, "--entrypoint", "sleep", "bench-wrk", "infinity"]);
  docker(["exec", name, "wrk", "-t1", "-c4", "-d2s", `http://${app.name}:8080/json`], { allowFail: true });
  return name;
}

async function wrk(app, test, opts, cpus, seconds) {
  const wrkName = app.wrk;
  const args = [
    "exec", wrkName, "wrk",
    `-t${opts.threads}`, `-c${opts.connections}`, `-d${seconds}s`, "--timeout", "10s",
    "-s", `/scripts/${test.script}`,
  ];
  if (test.auth) args.push("-H", `Authorization: Bearer ${TOKEN}`);
  args.push(`http://${app.name}:8080${test.path}`);

  const samples = [];
  let sampling = true;
  const sampler = (async () => {
    await sleep(1000);
    while (sampling) {
      const s = await statsAll([app.name, "bench-pg", wrkName]);
      if (s[app.name]) samples.push({ ...s[app.name], pgCpu: s["bench-pg"]?.cpu ?? null, wrkCpu: s[wrkName]?.cpu ?? null });
      await sleep(500);
    }
  })();

  const output = await new Promise((resolveRun, rejectRun) => {
    const child = spawn("docker", args);
    let out = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (out += d));
    child.on("error", rejectRun);
    child.on("close", () => resolveRun(out));
  });
  sampling = false;
  await sampler;

  const line = output.split("\n").find((l) => l.startsWith("BENCH_RESULT "));
  if (!line) throw new Error(`wrk produced no result:\n${output.slice(-2000)}`);
  const r = JSON.parse(line.slice("BENCH_RESULT ".length));
  const errors = Object.values(r.errors).reduce((a, b) => a + b, 0);
  return {
    rps: r.requests / (r.duration_us / 1e6),
    requests: r.requests,
    errors,
    error_detail: r.errors,
    latency_ms: Object.fromEntries(Object.entries(r.latency_us).map(([k, v]) => [k, v / 1000])),
    peak_mem_mb: samples.length ? Math.max(...samples.map((s) => s.memMb ?? 0)) : null,
    avg_cpu_pct: samples.length ? samples.reduce((a, s) => a + (s.cpu || 0), 0) / samples.length : null,
    pg_avg_cpu_pct: samples.length ? samples.reduce((a, s) => a + (s.pgCpu || 0), 0) / samples.length : null,
    wrk_avg_cpu_pct: samples.length ? samples.reduce((a, s) => a + (s.wrkCpu || 0), 0) / samples.length : null,
  };
}

// ---------------------------------------------------------------- one framework

async function benchFramework(fw, opts, cpus) {
  const db = opts.db ?? (opts.smoke ? `bench_${fw.name.replace(/-/g, "_")}` : "bench");
  const result = {
    name: fw.name,
    label: fw.label,
    language: fw.language,
    category: fw.category,
    mode: opts.smoke ? "smoke" : "full",
    started_at: new Date().toISOString(),
    settings: { runs: opts.runs, duration_s: opts.duration, warmup_s: opts.warmup, connections: opts.connections, threads: opts.threads, cpus },
  };
  let app;
  try {
    buildImage(fw, opts);
    const size = docker(["image", "inspect", "-f", "{{.Size}}", `bench-${fw.name}`]).stdout.trim();
    result.image_mb = Number(size) / 1048576;

    await resetDatabase(db);
    app = startApp(fw, db, cpus);
    result.startup_ms = await waitReady(app);
    await sleep(2000);
    result.idle_mem_mb = (await stats(app.name))?.memMb ?? null;
    log(`${fw.name}: ready in ${result.startup_ms} ms, idle ${result.idle_mem_mb?.toFixed(0)} MB`);

    const conformance = runConformance(app);
    result.conformance = conformance;
    log(`${fw.name}: conformance ${conformance.ok ? "PASS" : "FAIL"}`);
    if (!conformance.ok) {
      console.log(conformance.output);
      if (!opts.smoke) throw new Error("conformance failed; not benchmarked");
    }

    app.wrk = startLoadGenerator(app, cpus);
    result.tests = {};
    for (const test of TESTS) {
      if (opts.warmup > 0) await wrk(app, test, opts, cpus, opts.warmup);
      const runs = [];
      for (let i = 0; i < opts.runs; i++) runs.push(await wrk(app, test, opts, cpus, opts.duration));
      const sorted = [...runs].sort((a, b) => a.rps - b.rps);
      const median = sorted[Math.floor(sorted.length / 2)];
      result.tests[test.key] = {
        ...median,
        peak_mem_mb: Math.max(...runs.map((r) => r.peak_mem_mb ?? 0)),
        runs: runs.map((r) => ({ rps: r.rps, errors: r.errors, p99_ms: r.latency_ms.p99 })),
      };
      const cpuNote = `, app CPU ${Math.round(median.avg_cpu_pct ?? 0)}%, pg CPU ${Math.round(median.pg_avg_cpu_pct ?? 0)}%, wrk CPU ${Math.round(median.wrk_avg_cpu_pct ?? 0)}%`;
      const errorNote = median.errors ? `, ${median.errors} errors ${JSON.stringify(median.error_detail)}` : "";
      log(`${fw.name} ${test.key}: ${Math.round(median.rps).toLocaleString("en-US")} req/s, p99 ${median.latency_ms.p99.toFixed(1)} ms${cpuNote}${errorNote}`);
    }
    result.ok = true;
  } catch (error) {
    result.ok = false;
    result.error = error.message;
    log(`${fw.name}: FAILED: ${error.message.split("\n")[0]}`);
  } finally {
    if (app) {
      const logs = docker(["logs", "--tail", "500", app.name], { allowFail: true });
      writeFileSync(join(logDir(opts), `${fw.name}.log`), `${logs.stdout}\n${logs.stderr}`);
      docker(["rm", "-f", app.name], { allowFail: true });
      if (app.wrk) docker(["rm", "-f", app.wrk], { allowFail: true });
    }
    if (opts.smoke) psql("postgres", `DROP DATABASE IF EXISTS ${db} WITH (FORCE)`, true);
  }
  result.finished_at = new Date().toISOString();
  writeFileSync(join(resultDir(opts), `${fw.name}.json`), JSON.stringify(result, null, 2));
  return result;
}

function resultDir(opts) {
  const dir = join(BENCH, "results", opts.smoke ? "smoke" : "");
  mkdirSync(dir, { recursive: true });
  return dir;
}

function logDir(opts) {
  const dir = join(resultDir(opts), "logs");
  mkdirSync(dir, { recursive: true });
  return dir;
}

// ---------------------------------------------------------------- summary

const fmt = (n) => (n == null ? "–" : Math.round(n).toLocaleString("en-US"));

export function writeSummary(dir = join(BENCH, "results")) {
  if (!existsSync(dir)) return;
  const results = readdirSync(dir)
    .filter((f) => f.endsWith(".json") && f !== "env.json")
    .map((f) => JSON.parse(readFileSync(join(dir, f), "utf8")));
  const order = FRAMEWORKS.map((f) => f.name);
  results.sort((a, b) => order.indexOf(a.name) - order.indexOf(b.name));

  let md = "# Benchmark results\n\n";
  const s = results.find((r) => r.settings)?.settings ?? {};
  md += `Requests per second (median of ${s.runs ?? "the"} measured runs), higher is better. \`wrk\` ${s.threads ?? "?"} threads, ${s.connections ?? "?"} connections; the app limited to CPUs ${s.cpus?.app ?? "?"} and 4 GB, PostgreSQL CPUs ${s.cpus?.pg ?? "?"}, wrk CPUs ${s.cpus?.load ?? "?"}.\n\n`;
  md += `| Framework | Language | ${TESTS.map((t) => t.label).join(" | ")} | Startup (ms) | Idle mem (MB) | Peak mem (MB) |\n`;
  md += `|---|---|${TESTS.map(() => "---:").join("|")}|---:|---:|---:|\n`;
  for (const r of results) {
    if (!r.ok) {
      md += `| ${r.label} | ${r.language} | ${TESTS.map(() => "failed").join(" | ")} | | | |\n`;
      continue;
    }
    const peak = Math.max(...Object.values(r.tests).map((t) => t.peak_mem_mb ?? 0));
    md += `| ${r.label} | ${r.language} | ${TESTS.map((t) => fmt(r.tests[t.key]?.rps)).join(" | ")} | ${fmt(r.startup_ms)} | ${fmt(r.idle_mem_mb)} | ${fmt(peak)} |\n`;
  }
  md += "\n## p99 latency (ms)\n\n";
  md += `| Framework | ${TESTS.map((t) => t.label).join(" | ")} |\n|---|${TESTS.map(() => "---:").join("|")}|\n`;
  for (const r of results.filter((x) => x.ok)) {
    md += `| ${r.label} | ${TESTS.map((t) => r.tests[t.key]?.latency_ms.p99.toFixed(1) ?? "–").join(" | ")} |\n`;
  }
  const errored = results.filter((r) => r.ok && TESTS.some((t) => r.tests[t.key]?.errors));
  if (errored.length) {
    md += "\n## Errors during measured runs\n\n";
    for (const r of errored) {
      for (const t of TESTS) {
        const e = r.tests[t.key];
        if (e?.errors) md += `- ${r.label}, ${t.label}: ${e.errors} of ${e.requests} (${JSON.stringify(e.error_detail)})\n`;
      }
    }
  }
  const failed = results.filter((r) => !r.ok);
  if (failed.length) {
    md += "\n## Not benchmarked\n\n";
    for (const r of failed) md += `- ${r.label}: ${r.error?.split("\n")[0]}\n`;
  }
  writeFileSync(join(dir, "summary.md"), md);
  return md;
}

// ---------------------------------------------------------------- main

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.report) {
    console.log(writeSummary());
    return;
  }
  const selected = opts.only ? FRAMEWORKS.filter((f) => opts.only.includes(f.name)) : FRAMEWORKS;
  if (opts.only && selected.length !== opts.only.length) {
    throw new Error(`unknown framework in --only (known: ${FRAMEWORKS.map((f) => f.name).join(", ")})`);
  }
  const cpus = cpuLayout(opts);
  log(`CPUs: ${cpus.ncpu} (app ${cpus.app}, postgres ${cpus.pg}, wrk ${cpus.load})`);
  ensureNetwork();
  await ensurePostgres(cpus);
  ensureWrkImage();
  if (!opts.smoke) {
    const version = docker(["version", "--format", "{{.Server.Version}}"]).stdout.trim();
    writeFileSync(join(resultDir(opts), "env.json"), JSON.stringify({ docker: version, cpus, date: new Date().toISOString() }, null, 2));
  }

  for (const fw of selected) {
    if (opts.skipDone && existsSync(join(resultDir(opts), `${fw.name}.json`))) {
      const prev = JSON.parse(readFileSync(join(resultDir(opts), `${fw.name}.json`), "utf8"));
      if (prev.ok) {
        log(`${fw.name}: already done, skipping`);
        continue;
      }
    }
    log(`=== ${fw.label} ===`);
    await benchFramework(fw, opts, cpus);
    if (!opts.smoke) writeSummary();
  }
  if (!opts.smoke) console.log(`\n${writeSummary()}`);
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
