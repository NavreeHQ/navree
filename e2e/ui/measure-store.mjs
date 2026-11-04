// Measures the store app at full size and writes what it found for the website's
// store page: how long the seed takes, how many rows it writes, how fast the first
// screen of each page paints, how many bytes each sends, and the plan SQLite
// chooses for every statement behind the orders list, a filtered search and the
// order drawer.
//
//   cargo build --release -p store-demo
//   node measure-store.mjs                 # writes apps/website/content/showcase/store.json
//
// MEASURE_BIN names the binary (default ../../target/release/store-demo[.exe]),
// MEASURE_PORT its port (8096), MEASURE_RUNS the runs per page (5; the median is
// kept). It starts the app itself, twice, with its simulator off, on a database of
// its own that it removes afterwards: once as it runs for the timings, and once
// logging every statement, for the plans.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { cpus, platform, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "../..");
const bin = process.env.MEASURE_BIN || join(repo, "target/release", platform() === "win32" ? "store-demo.exe" : "store-demo");
const port = Number(process.env.MEASURE_PORT || 8096);
const runs = Number(process.env.MEASURE_RUNS || 5);
const base = `http://127.0.0.1:${port}`;
const dir = join(tmpdir(), `store-measure-${process.pid}`);
const db = join(dir, "store.db");
const out = join(repo, "apps/website/content/showcase/store.json");
mkdirSync(dir, { recursive: true });

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
};
const round = (n) => Math.round(n);

// ---- The app, started here; logging every statement it runs when asked ------------

const statements = [];
let mark = 0;

/** Starts the app on a fresh database and resolves once it answers, with how long that took. */
async function start(log) {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const began = performance.now();
  const app = spawn(bin, [], {
    env: {
      ...process.env,
      PORT: String(port),
      STORE_SIMULATOR: "off",
      ...(log ? { STORE_LOG_QUERIES: "1" } : {}),
      STORE_DATABASE_URL: `sqlite://${db.replaceAll("\\", "/")}?mode=rwc`,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let buffer = "";
  const take = (chunk) => {
    if (!log) return;
    buffer += chunk;
    let at;
    while ((at = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, at);
      buffer = buffer.slice(at + 1);
      if (!line.includes("navree_db::query") || line.includes("INSERT INTO")) continue;
      try {
        const entry = JSON.parse(line);
        const fields = entry.fields ?? entry;
        if (fields.sql) statements.push({ sql: String(fields.sql), ms: Number(fields.elapsed_ms ?? 0) });
      } catch {}
    }
  };
  app.stdout.on("data", take);
  app.stderr.on("data", take);
  for (;;) {
    try {
      if ((await fetch(`${base}/orders`)).ok) break;
    } catch {}
    if (app.exitCode !== null) throw new Error(`the app stopped: ${app.exitCode}`);
    await new Promise((r) => setTimeout(r, 250));
  }
  return { app, seconds: (performance.now() - began) / 1000 };
}

/** Stops the app and waits for it. */
async function stop(app) {
  app.kill();
  if (app.exitCode === null) await new Promise((r) => app.once("exit", r));
}

const first = await start(false);
const seedSeconds = first.seconds;

// ---- How many rows, and how big ---------------------------------------------------

let sqlite = new DatabaseSync(db, { readOnly: true });
const count = (table) => Number(sqlite.prepare(`SELECT COUNT(*) AS n FROM ${table}`).get().n);
const rows = {
  orders: count("orders"),
  lines: count("order_lines"),
  events: count("events"),
  customers: count("customers"),
  products: count("products"),
  bytes: statSync(db).size + (existsSync(`${db}-wal`) ? statSync(`${db}-wal`).size : 0),
  seed_seconds: Math.round(seedSeconds * 10) / 10,
};

// ---- First screen and bytes, in Chromium, on a fast machine and a slow phone ------

const profiles = {
  desktop: null,
  // Lighthouse's mobile: 4x slower CPU and Slow 4G.
  phone: { cpu: 4, latency: 150, down: (1.6 * 1024 * 1024) / 8, up: (750 * 1024) / 8 },
};
const browser = await chromium.launch();

/** Loads `path` and says when it painted and what it sent. */
async function load(path, profile, viewport) {
  const context = await browser.newContext({ viewport });
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Network.setCacheDisabled", { cacheDisabled: true });
  if (profile) {
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: profile.cpu });
    await cdp.send("Network.emulateNetworkConditions", {
      offline: false,
      latency: profile.latency,
      downloadThroughput: profile.down,
      uploadThroughput: profile.up,
    });
  }
  await page.addInitScript(() => {
    window.__lcp = 0;
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) window.__lcp = entry.startTime;
    }).observe({ type: "largest-contentful-paint", buffered: true });
  });
  await page.goto(base + path, { waitUntil: "load" });
  await page.waitForTimeout(400);
  const timing = await page.evaluate(() => {
    const nav = performance.getEntriesByType("navigation")[0];
    const fcp = performance.getEntriesByName("first-contentful-paint")[0]?.startTime ?? 0;
    const resources = performance.getEntriesByType("resource");
    return {
      ttfb: nav.responseStart,
      fcp,
      lcp: window.__lcp || fcp,
      html: nav.transferSize,
      htmlDecoded: nav.decodedBodySize,
      total: nav.transferSize + resources.reduce((sum, r) => sum + (r.transferSize || 0), 0),
      requests: 1 + resources.length,
    };
  });
  await context.close();
  return timing;
}

const pages = [
  { name: "Dashboard", path: "/" },
  { name: "Orders", path: "/orders" },
  { name: "Filtered search", path: "/orders?filter%5Bstatus%5D%5Bin%5D=open&filter%5Bfulfilment%5D%5Bin%5D=packed&filter%5Bstate%5D%5Bin%5D=PB&search=CS-2" },
  { name: "An order's page", path: "/orders/150000" },
  { name: "Inventory", path: "/inventory" },
];
const measured = [];
for (const { name, path } of pages) {
  const result = { name, path };
  for (const [label, profile] of Object.entries(profiles)) {
    const viewport = label === "phone" ? { width: 390, height: 844 } : { width: 1440, height: 900 };
    const samples = [];
    for (let i = 0; i < runs; i++) samples.push(await load(path, profile, viewport));
    result[label] = {
      ttfb_ms: round(median(samples.map((s) => s.ttfb))),
      fcp_ms: round(median(samples.map((s) => s.fcp))),
      lcp_ms: round(median(samples.map((s) => s.lcp))),
      html_bytes: median(samples.map((s) => s.html)),
      html_decoded_bytes: median(samples.map((s) => s.htmlDecoded)),
      total_bytes: median(samples.map((s) => s.total)),
      requests: median(samples.map((s) => s.requests)),
    };
  }
  measured.push(result);
}

// ---- The drawer: from the click to the drawer, and to its slow parts --------------

const drawerSamples = [];
for (let i = 0; i < runs; i++) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  await page.goto(`${base}/orders`, { waitUntil: "load" });
  const button = page.locator("#orders tbody tr[data-key] button[aria-haspopup=dialog]").nth(i);
  const bytesBefore = await page.evaluate(() => performance.getEntriesByType("resource").reduce((s, r) => s + (r.transferSize || 0), 0));
  const t0 = await page.evaluate(() => performance.now());
  await button.click();
  await page.locator("dialog#order[open]").waitFor();
  const opened = await page.evaluate(() => performance.now());
  await page.waitForFunction(() => !document.querySelector("dialog#order [aria-busy=true]"));
  const loaded = await page.evaluate(() => performance.now());
  const bytesAfter = await page.evaluate(() => performance.getEntriesByType("resource").reduce((s, r) => s + (r.transferSize || 0), 0));
  drawerSamples.push({ open: opened - t0, parts: loaded - t0, bytes: bytesAfter - bytesBefore });
  await context.close();
}
const drawer = {
  open_ms: round(median(drawerSamples.map((s) => s.open))),
  parts_ms: round(median(drawerSamples.map((s) => s.parts))),
  bytes: median(drawerSamples.map((s) => s.bytes)),
};
await browser.close();

// ---- The statements behind three views, and the plan for each ---------------------

sqlite.close();
await stop(first.app);
const second = await start(true);
sqlite = new DatabaseSync(db, { readOnly: true });

/** What `path` runs, read from the log: every statement logged while it loads. */
async function statementsOf(path) {
  await new Promise((r) => setTimeout(r, 300));
  mark = statements.length;
  const response = await fetch(base + path, { headers: path.endsWith("/peek") ? { "x-navree-ui": "1" } : {} });
  await response.text();
  await new Promise((r) => setTimeout(r, 300));
  return statements.slice(mark);
}
const plan = (sql) => {
  try {
    return sqlite
      .prepare(`EXPLAIN QUERY PLAN ${sql}`)
      .all()
      .map((row) => row.detail);
  } catch (error) {
    return [`(no plan: ${error.message})`];
  }
};
const views = [
  { name: "The orders list, first page", paths: ["/orders"] },
  { name: "A filtered search", paths: ["/orders?filter%5Bstatus%5D%5Bin%5D=open&filter%5Bfulfilment%5D%5Bin%5D=packed&filter%5Bstate%5D%5Bin%5D=PB&search=CS-2"] },
  { name: "The order drawer", paths: ["/orders/150000/peek", "/orders/150000/steps", "/orders/150000/others"] },
];
const plans = [];
for (const view of views) {
  const found = [];
  for (const path of view.paths) for (const statement of await statementsOf(path)) found.push({ path, ...statement });
  plans.push({
    name: view.name,
    statements: found
      .filter((s) => /^\s*SELECT/i.test(s.sql))
      .map((s) => ({ path: s.path, sql: s.sql, ms: Math.round(s.ms * 100) / 100, plan: plan(s.sql) })),
  });
}
sqlite.close();
await stop(second.app);
rmSync(dir, { recursive: true, force: true });

const result = {
  measured_on: new Date().toISOString().slice(0, 10),
  machine: `${cpus()[0]?.model.trim() ?? "unknown CPU"}, ${cpus().length} threads, ${platform()}`,
  method:
    "Release build, SQLite on local disk, Chromium driven by Playwright with the cache off; the median of " +
    `${runs} loads per page. The phone profile is 390 px wide, 4x slower CPU and Slow 4G (150 ms, 1.6 Mbit/s).`,
  rows,
  pages: measured,
  drawer,
  plans,
};
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(result, null, 2) + "\n");
console.log(JSON.stringify({ rows, drawer, pages: measured.map((p) => [p.name, p.desktop.lcp_ms, p.phone.lcp_ms, p.desktop.html_bytes]) }, null, 2));
for (const view of plans) {
  console.log(`\n${view.name}`);
  for (const s of view.statements) console.log(`  ${s.ms} ms  ${s.sql.slice(0, 120)}\n    ${s.plan.join("\n    ")}`);
}
