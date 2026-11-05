/**
 * Serves `out/` with `scripts/serve.ts` for a check that needs the built site:
 * on a port the system says is free (a port picked at random can be taken, and
 * the server then exits at once), waiting until it answers.
 */

import { spawn, type ChildProcess } from "node:child_process";
import http from "node:http";
import net from "node:net";
import path from "node:path";

import { WEBSITE } from "./site-html.ts";

/**
 * How long a server that was started may take to answer. A cold machine is slow, so this
 * ends a hang and is not a deadline: the script waits for the server to answer, however
 * long that takes within it, and stops at once if the server exits.
 */
const SERVER_START_LIMIT_MS = 120_000;

/** Whether `url` answers with anything but a server error. */
export const ready = (url: string) =>
  new Promise<boolean>((resolve) => {
    const request = http
      .get(url, (res) => {
        res.resume();
        resolve((res.statusCode ?? 500) < 500);
      })
      .on("error", () => resolve(false));
    // A server that takes the connection and never answers is not ready; ask again.
    request.setTimeout(2000, () => request.destroy());
  });

/** A port nothing listens on: the system picks it. */
const freePort = () =>
  new Promise<number>((resolve, reject) => {
    const probe = net.createServer();
    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address() as net.AddressInfo;
      probe.close(() => resolve(port));
    });
  });

async function waitUntilServing(script: string, base: string, server: ChildProcess) {
  const exit: { reason: string | null } = { reason: null };
  server.once("exit", (code, signal) => {
    exit.reason = `code ${code}, signal ${signal}`;
  });
  const deadline = Date.now() + SERVER_START_LIMIT_MS;
  while (!(await ready(`${base}/`))) {
    if (exit.reason) throw new Error(`${script}: the server stopped before it answered (${exit.reason})`);
    if (Date.now() > deadline) throw new Error(`${script}: the server at ${base} did not answer within ${SERVER_START_LIMIT_MS / 1000} s`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

/** Starts `scripts/serve.ts` on a free port and returns its address once it answers; `script` names the caller in errors. */
export async function serveOut(script: string): Promise<{ base: string; server: ChildProcess }> {
  const port = String(await freePort());
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(process.execPath, [path.join(WEBSITE, "scripts", "serve.ts")], { env: { ...process.env, PORT: port }, stdio: ["ignore", "ignore", "inherit"] });
  try {
    await waitUntilServing(script, base, server);
  } catch (error) {
    server.kill();
    throw error;
  }
  return { base, server };
}
