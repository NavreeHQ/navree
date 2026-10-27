// Builds test/fixture (a Next.js app on @navree/next) and checks the running server against a
// scripted Navree API: the proxy (with a token refresh), server components with auth() and
// client(), the client components, and the BFF routes.
//
//   pnpm --filter @navree/next smoke          (build + check)
//   node test/smoke.mjs --no-build             (check an existing build)
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";
import { readSetCookies, serveFakeNavree, tokenPair, unsignedJwt } from "@navree/bff/testing";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = join(root, "test", "fixture");
const port = 3800 + Math.floor(Math.random() * 90);
const base = `http://127.0.0.1:${port}`;
const next = join(root, "node_modules", ".bin", process.platform === "win32" ? "next.CMD" : "next");
const user = { __type: "User", id: "u1", name: "Ada" };
const unauthorized = () =>
  Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized", code: "unauthenticated" }, { status: 401 });

if (!process.argv.includes("--no-build")) {
  const build = spawnSync(`"${next}" build "${fixture}"`, { cwd: root, stdio: "inherit", shell: true, env: { ...process.env, NEXT_TELEMETRY_DISABLED: "1" } });
  if (build.status !== 0) process.exit(build.status ?? 1);
}

const api = await serveFakeNavree({
  "POST /api/auth/login": () => Response.json({ user, ...tokenPair(1) }),
  "POST /api/auth/refresh": (call) => (JSON.parse(call.body).refresh_token === "RT0" ? Response.json(tokenPair(1)) : unauthorized()),
  "GET /api/auth/me": (call) => (call.headers.get("authorization") === "Bearer AT1" ? Response.json(user) : unauthorized()),
  "GET /api/v1/deals": (call) => Response.json({ auth: call.headers.get("authorization"), workspace: call.headers.get("x-workspace") }),
  "GET /admin/users": () =>
    new Response("<h1>Users</h1>", { headers: { "content-type": "text/html", "content-security-policy": "default-src 'self'" } }),
  "POST /broadcasting/auth": () => Response.json({ auth: "app-key:sig" }),
});

const env = { ...process.env, NEXT_TELEMETRY_DISABLED: "1", NAVREE_API_URL: api.url, NAVREE_COOKIE_SECURE: "0" };
const server = spawn(`"${next}" start "${fixture}" -p ${port} -H 127.0.0.1`, { cwd: root, env, shell: true, stdio: ["ignore", "pipe", "inherit"] });

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
const json = { origin: base, "content-type": "application/json" };
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

check("sign-in sets httpOnly token cookies and returns only the user", async () => {
  const response = await fetch(`${base}/api/auth/login`, { method: "POST", headers: json, body: JSON.stringify({ email: "a", password: "b" }) });
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { user });
  const cookies = readSetCookies(response);
  assert.equal(cookies.navree_at.value, "AT1");
  assert.ok(cookies.navree_rt.attrs.includes("HttpOnly"));
});

check("server components get the user from auth() and call the API with client()", async () => {
  const response = await fetch(`${base}/?filter[stage]=won`, { headers: { ...html, cookie: "navree_at=AT1; navree_workspace=acme" } });
  assert.equal(response.status, 200);
  const page = await response.text();
  assert.match(page, /user:Ada/);
  assert.match(page, /ws:acme auth:Bearer AT1/);
  assert.match(page, /data-slot="data-table"/);
  assert.match(page, /Acme renewal/);
  assert.match(page, /can read/);
  assert.match(page, /<html lang="en" dir="ltr"/);
});

check("the proxy refreshes an expired token, and the page renders with the new one", async () => {
  const stale = unsignedJwt({ sub: "u1", exp: Math.floor(Date.now() / 1000) - 60 });
  const response = await fetch(`${base}/`, { headers: { ...html, cookie: `navree_at=${stale}; navree_rt=RT0` } });
  assert.equal(response.status, 200);
  assert.equal(readSetCookies(response).navree_at.value, "AT1");
  assert.match(await response.text(), /user:Ada/);
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

check("a mounted prefix proxies a server-rendered page with its security headers", async () => {
  const response = await fetch(`${base}/admin/users`, { headers: { cookie: "navree_at=AT1" } });
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("content-security-policy"), "default-src 'self'");
  assert.match(await response.text(), /Users/);
});

check("channel authorization goes through the BFF", async () => {
  const response = await fetch(`${base}/broadcasting/auth`, {
    method: "POST",
    headers: { ...json, cookie: "navree_at=AT1" },
    body: JSON.stringify({ socket_id: "1.2", channel_name: "private-crm.deals" }),
  });
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { auth: "app-key:sig" });
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
  await api.close();
}
console.log(failed === 0 ? `\n${checks.length} checks passed` : `\n${failed} of ${checks.length} checks failed`);
process.exit(failed === 0 ? 0 : 1);
