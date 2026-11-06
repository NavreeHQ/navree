// Serves the built export (apps/website/out) with apps/website/scripts/serve.ts
// on a free port, for the tests of this folder. SITE_URL (an origin, such as
// http://127.0.0.1:3211) points at a server that is already running instead.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const WEBSITE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..", "apps", "website");

/** A port nothing listens on. */
const freePort = () =>
  new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
  });

/** Whether `url` answers with anything but a server error. */
const ready = (url) =>
  new Promise((resolve) => {
    const request = http
      .get(url, (res) => {
        res.resume();
        resolve((res.statusCode ?? 500) < 500);
      })
      .on("error", () => resolve(false));
    request.setTimeout(2000, () => request.destroy());
  });

/**
 * The origin of the site and a function that stops the server this call
 * started, if it started one. Waits for the server to answer; a server that
 * stops first fails the call.
 */
export async function serve() {
  if (process.env.SITE_URL) return { origin: process.env.SITE_URL.replace(/\/$/, ""), stop() {} };
  assert.ok(fs.existsSync(path.join(WEBSITE, "out", "index.html")), "apps/website/out is missing: run `pnpm build` in apps/website first");
  const port = await freePort();
  const server = spawn(process.execPath, [path.join(WEBSITE, "scripts", "serve.ts")], { env: { ...process.env, PORT: String(port) }, stdio: ["ignore", "ignore", "inherit"] });
  const origin = `http://127.0.0.1:${port}`;
  for (const end = Date.now() + 120_000; !(await ready(`${origin}/`)); ) {
    assert.ok(Date.now() < end, `the server at ${origin} did not answer within 120 s`);
    assert.equal(server.exitCode, null, "the server stopped before it answered");
    await new Promise((r) => setTimeout(r, 100));
  }
  return { origin, stop: () => server.kill() };
}
