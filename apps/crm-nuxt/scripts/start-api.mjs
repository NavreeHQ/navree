#!/usr/bin/env node
// Runs crm-api (the Navree CRM API) for this front end, on SQLite, without touching
// apps/crm-api's own database or .env.
//
//   node scripts/start-api.mjs              # serve on http://127.0.0.1:8092 (seeds a new database first)
//   node scripts/start-api.mjs --fresh      # migrate:fresh + db:seed crm-demo, then serve
//   node scripts/start-api.mjs --worker     # also run queue:work (deal notifications, mail to the log)
//   node scripts/start-api.mjs --db e2e     # use target/builds/crm-nuxt/e2e.db instead of crm.db
//
// The binary: $CRM_API_BIN, else target/builds/crm-nuxt/crm-api.exe (crm-api on Linux and macOS),
// else a copy of target/builds/patterns/debug/crm-api(.exe). Build one with
//   CARGO_TARGET_DIR=target/builds/crm-nuxt cargo build -p crm-api
// and point CRM_API_BIN at target/builds/crm-nuxt/debug/crm-api(.exe).
//
// Environment: CRM_API_PORT (8092), CRM_WEB_ORIGIN (http://localhost:3102), APP_LOG__LEVEL (warn).
import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");
const apiDir = path.join(repoRoot, "apps/crm-api");
const stateDir = path.join(repoRoot, "target/builds/crm-nuxt");
const exe = process.platform === "win32" ? "crm-api.exe" : "crm-api";

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const option = (name, fallback) => {
  const index = args.indexOf(name);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};

mkdirSync(stateDir, { recursive: true });

function binary() {
  if (process.env.CRM_API_BIN) return process.env.CRM_API_BIN;
  const own = path.join(stateDir, exe);
  if (!existsSync(own)) {
    const shared = path.join(repoRoot, "target/builds/patterns/debug", exe);
    if (!existsSync(shared)) {
      console.error(`[crm-api] No binary at ${own}. Build one (see the top of this script) or set CRM_API_BIN.`);
      process.exit(1);
    }
    copyFileSync(shared, own);
  }
  return own;
}

/** One APP_KEY per checkout, kept next to the database so tokens survive restarts. */
function appKey() {
  const file = path.join(stateDir, "app-key.txt");
  if (existsSync(file)) return readFileSync(file, "utf8").trim();
  const key = `base64:${randomBytes(32).toString("base64")}`;
  writeFileSync(file, `${key}\n`);
  return key;
}

const port = Number(process.env.CRM_API_PORT ?? 8092);
const webOrigin = process.env.CRM_WEB_ORIGIN ?? "http://localhost:3102";
const dbName = option("--db", "crm");
const dbFile = path.join(stateDir, `${dbName}.db`);
// The API runs from apps/crm-api (it reads config/app.toml there); the database path is relative to it.
const dbUrl = `sqlite://${path.relative(apiDir, dbFile).split(path.sep).join("/")}?mode=rwc`;

const env = {
  ...process.env,
  APP_KEY: process.env.APP_KEY ?? appKey(),
  APP_DATABASE__URL: dbUrl,
  APP_HTTP__ADDR: `127.0.0.1:${port}`,
  APP_APP__URL: webOrigin,
  // The browser's socket connects to the API directly, from the front end's origin.
  APP_REALTIME__ALLOWED_ORIGINS: [webOrigin, webOrigin.replace("localhost", "127.0.0.1")].join(","),
  // Every browser reaches the API through the BFF, i.e. from 127.0.0.1: lift the per-IP limit locally.
  APP_HTTP__PER_IP_RATE_LIMIT: process.env.APP_HTTP__PER_IP_RATE_LIMIT ?? "100000",
  APP_LOG__LEVEL: process.env.APP_LOG__LEVEL ?? "warn",
};

const bin = binary();

function run(command) {
  console.log(`[crm-api] ${command.join(" ")}`);
  const result = spawnSync(bin, command, { cwd: apiDir, stdio: "inherit", env });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    console.error(`[crm-api] \`${command.join(" ")}\` failed with exit code ${result.status}`);
    process.exit(result.status ?? 1);
  }
}

if (flag("--fresh") || !existsSync(dbFile)) {
  run(["migrate:fresh"]);
  run(["db:seed", "crm-demo"]);
}

const children = [spawn(bin, ["serve"], { cwd: apiDir, stdio: "inherit", env })];
if (flag("--worker")) children.push(spawn(bin, ["queue:work"], { cwd: apiDir, stdio: "inherit", env }));
console.log(`[crm-api] http://127.0.0.1:${port} (database ${path.relative(repoRoot, dbFile)})`);

const stop = (signal) => {
  for (const child of children) child.kill(signal);
};
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => stop(signal));
children[0].on("exit", (code, signal) => {
  stop("SIGTERM");
  process.exit(code ?? (signal ? 1 : 0));
});
