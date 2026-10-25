// Starts crm-api for the Playwright suite (E2E_REAL_API=1) on a fresh, seeded database:
//
//   cargo run -p crm-api -- migrate:fresh
//   cargo run -p crm-api -- db:seed crm-demo
//   cargo run -p crm-api -- serve            (http://127.0.0.1:8080)
//
// Run from apps/crm-api (the API reads config/app.toml from its working directory).
// playwright.config.ts passes the environment: CARGO_TARGET_DIR, APP_KEY (tokens must
// survive the API restart between runs) and APP_DATABASE__URL (a database of its own,
// so the suite never touches storage/crm.db). The first run compiles the API, which
// can take several minutes.
import { spawn, spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const apiDir = path.resolve(here, "../../crm-api");

function cargoArgs(command) {
  return ["run", "--quiet", "-p", "crm-api", "--", ...command];
}

function prepare(command) {
  console.log(`[crm-api] cargo ${cargoArgs(command).join(" ")}`);
  const result = spawnSync("cargo", cargoArgs(command), { cwd: apiDir, stdio: "inherit", env: process.env });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    console.error(`[crm-api] \`${command.join(" ")}\` failed with exit code ${result.status}`);
    process.exit(result.status ?? 1);
  }
}

prepare(["migrate:fresh"]);
prepare(["db:seed", "crm-demo"]);

console.log("[crm-api] cargo run -p crm-api -- serve");
const server = spawn("cargo", cargoArgs(["serve"]), { cwd: apiDir, stdio: "inherit", env: process.env });
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.kill(signal));
}
server.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
