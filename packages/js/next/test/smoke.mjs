// Builds test/fixture (a Next.js app on @navree/next) and checks the running server: the
// proxy, server rendering with auth() and the client components, and the BFF routes.
//
//   pnpm --filter @navree/next smoke          (build + check)
//   node test/smoke.mjs --no-build             (check an existing build)
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = join(root, "test", "fixture");
const port = 3800 + Math.floor(Math.random() * 90);
const base = `http://127.0.0.1:${port}`;
const next = join(root, "node_modules", ".bin", process.platform === "win32" ? "next.CMD" : "next");
const env = { ...process.env, NEXT_TELEMETRY_DISABLED: "1", NAVREE_API_URL: "http://127.0.0.1:9" };
const shell = process.platform === "win32";

if (!process.argv.includes("--no-build")) {
  const build = spawnSync(next, ["build", fixture], { cwd: root, stdio: "inherit", env, shell });
  if (build.status !== 0) process.exit(build.status ?? 1);
}

const server = spawn(next, ["start", fixture, "-p", String(port), "-H", "127.0.0.1"], { cwd: root, env, shell, stdio: ["ignore", "pipe", "inherit"] });

async function ready() {
  for (let i = 0; i < 200; i++) {
    try {
      await fetch(`${base}/about`);
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  throw new Error("the server did not start");
}

const html = { accept: "text/html" };
const checks = [];
const check = (name, fn) => checks.push([name, fn]);

check("a public page renders", async () => {
  const response = await fetch(`${base}/about`, { headers: html });
  assert.equal(response.status, 200);
  assert.match(await response.text(), /About/);
});

check("the proxy sends a page behind sign-in to /login?next=", async () => {
  const response = await fetch(`${base}/?tab=open`, { headers: html, redirect: "manual" });
  assert.equal(response.status, 307);
  const location = new URL(response.headers.get("location"), base);
  assert.equal(location.pathname + location.search, "/login?next=%2F%3Ftab%3Dopen");
});

check("the sign-in page is public", async () => {
  const response = await fetch(`${base}/login`, { headers: html });
  assert.equal(response.status, 200);
  assert.match(await response.text(), /Sign in/);
});

check("a page with a session renders auth(), the abilities and the data table", async () => {
  const response = await fetch(`${base}/?filter[stage]=won`, { headers: { ...html, cookie: "navree_at=opaque" } });
  assert.equal(response.status, 200);
  const page = await response.text();
  assert.match(page, /Deals/);
  assert.match(page, /unconfirmed/);
  assert.match(page, /data-slot="data-table"/);
  assert.match(page, /Acme renewal/);
  assert.match(page, /can read/);
  assert.match(page, /<html lang="en" dir="ltr"/);
});

check("the BFF refuses a write from another origin", async () => {
  const response = await fetch(`${base}/api/auth/login`, {
    method: "POST",
    headers: { origin: "https://evil.example", "content-type": "application/json" },
    body: "{}",
  });
  assert.equal(response.status, 403);
  assert.equal((await response.json()).code, "invalid_origin");
});

check("the BFF answers 502 api_unreachable when the API is down", async () => {
  for (const [path, init] of [
    ["/api/auth/login", { method: "POST", headers: { origin: base, "content-type": "application/json" }, body: "{}" }],
    ["/api/v1/deals", {}],
    ["/admin/users", {}],
    ["/broadcasting/auth", { method: "POST", headers: { origin: base, "content-type": "application/json" }, body: "{}" }],
  ]) {
    const response = await fetch(`${base}${path}`, init);
    assert.equal(response.status, 502, path);
    assert.equal((await response.json()).code, "api_unreachable", path);
  }
});

let failed = 0;
try {
  await ready();
  for (const [name, fn] of checks) {
    try {
      await fn();
      console.log(`ok   ${name}`);
    } catch (error) {
      failed += 1;
      console.log(`FAIL ${name}\n     ${error.message}`);
    }
  }
} finally {
  if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(server.pid), "/T", "/F"], { stdio: "ignore" });
  else server.kill();
}
console.log(failed === 0 ? `\n${checks.length} checks passed` : `\n${failed} of ${checks.length} checks failed`);
process.exit(failed === 0 ? 0 : 1);
