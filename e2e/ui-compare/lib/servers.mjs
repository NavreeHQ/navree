// The two things the harness may have to start: the proof app that serves the Navree
// pages, and a static host for the built React pages. Whatever is started here is
// stopped here, by the process it was started as and no other.
import { spawn, spawnSync } from "node:child_process";
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { createServer } from "node:http";
import net from "node:net";
import { extname, join, posix, resolve, sep } from "node:path";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";
import { sleep } from "./stats.mjs";

const isWindows = process.platform === "win32";

/** A TCP port nothing is listening on. */
export function freePort() {
  return new Promise((resolvePort, reject) => {
    const probe = net.createServer();
    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => resolvePort(port));
    });
  });
}

/** Fetches `url` and answers its status, or 0 when nothing answers. */
async function status(url) {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(5000) });
    await response.arrayBuffer();
    return response.status;
  } catch {
    return 0;
  }
}

/** Polls until `url` answers 200, or `child` exits, or `limit` ms pass. */
async function waitUntilServing(url, child, limit, output) {
  const until = Date.now() + limit;
  while (Date.now() < until) {
    if (child && child.exitCode !== null) {
      throw new Error(`the proof app stopped (exit code ${child.exitCode}) before it answered:\n${output().slice(-2000)}`);
    }
    if ((await status(url)) === 200) return;
    await sleep(250);
  }
  throw new Error(`${url} did not answer 200 within ${Math.round(limit / 1000)} s:\n${output().slice(-2000)}`);
}

/** Runs a command to its end with its output on this terminal; throws if it fails. */
export function run(command, args, { cwd, env, shell = false } = {}) {
  const result = spawnSync(command, args, { cwd, env: { ...process.env, ...env }, stdio: "inherit", shell });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed with exit code ${result.status}`);
}

/**
 * Builds the proof app in release mode and answers the path of its program, read from
 * what cargo reports about the artifact it made.
 */
function buildProofApp(root) {
  const cargo = process.env.CARGO || "cargo";
  const result = spawnSync(cargo, ["build", "--release", "-p", "ui-proof", "--message-format=json-render-diagnostics"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 512 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`cargo build of the proof app failed with exit code ${result.status}`);
  let program = null;
  for (const line of result.stdout.split("\n")) {
    if (!line.startsWith("{")) continue;
    try {
      const message = JSON.parse(line);
      if (message.reason === "compiler-artifact" && message.target?.name === "ui-proof" && message.executable) {
        program = message.executable;
      }
    } catch {
      // A line that is not one of cargo's messages.
    }
  }
  if (program && existsSync(program)) return program;
  const fallback = join(process.env.CARGO_TARGET_DIR || join(root, "target"), "release", isWindows ? "ui-proof.exe" : "ui-proof");
  if (existsSync(fallback)) return fallback;
  throw new Error("cargo built the proof app but did not say where the program is");
}

/**
 * The Navree pages. With `NAVREE_UI_URL` set, that server is used and left alone.
 * Otherwise the proof app is built and started on a free port, and stopped by `stop()`.
 */
export async function startNavree({ root, log }) {
  const external = process.env.NAVREE_UI_URL;
  if (external) {
    const url = external.replace(/\/+$/, "");
    log(`Using the Navree UI server at ${url} (NAVREE_UI_URL); it is left running.`);
    return { url, pid: null, stop: async () => {} };
  }
  log("Building the proof app (cargo build --release -p ui-proof) ...");
  const program = buildProofApp(root);
  const port = await freePort();
  const url = `http://127.0.0.1:${port}`;
  let tail = "";
  const child = spawn(program, ["serve"], {
    cwd: root,
    env: { ...process.env, PORT: String(port) },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const remember = (chunk) => {
    tail = (tail + chunk.toString()).slice(-8000);
  };
  child.stdout.on("data", remember);
  child.stderr.on("data", remember);
  child.on("error", (error) => {
    tail += `\n${error}`;
  });
  log(`Started the proof app as process ${child.pid}, serving ${url}`);
  const stop = async () => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.kill();
    for (let waited = 0; waited < 10000 && child.exitCode === null && child.signalCode === null; waited += 100) {
      await sleep(100);
    }
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  };
  try {
    await waitUntilServing(`${url}/compare/table`, child, 120000, () => tail);
  } catch (error) {
    await stop();
    throw error;
  }
  return { url, pid: child.pid, stop, child };
}

// ------------------------------------------------------------ static host

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".txt": "text/plain; charset=utf-8",
  ".woff2": "font/woff2",
  ".png": "image/png",
  ".ico": "image/x-icon",
};
const TEXT = new Set([".html", ".js", ".mjs", ".css", ".json", ".svg", ".txt"]);

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* walk(path);
    else yield path;
  }
}

/**
 * Serves a built folder the way a normal host does: text files compressed ahead of time
 * at brotli 11 and gzip 9 (the level the Navree server compresses its files at), chosen
 * by what the browser accepts; files under `assets/` cached for a year as they have
 * hashed names, pages not cached.
 */
export async function serveFolder(folder) {
  const base = resolve(folder);
  const files = new Map();
  for (const path of walk(base)) {
    const url = "/" + path.slice(base.length + 1).split(sep).join("/");
    const extension = extname(path).toLowerCase();
    const body = readFileSync(path);
    const entry = { type: TYPES[extension] || "application/octet-stream", body, br: null, gzip: null, url };
    if (TEXT.has(extension) && body.length > 256) {
      entry.br = brotliCompressSync(body, {
        params: {
          [constants.BROTLI_PARAM_QUALITY]: 11,
          [constants.BROTLI_PARAM_LGWIN]: 22,
          [constants.BROTLI_PARAM_SIZE_HINT]: body.length,
        },
      });
      entry.gzip = gzipSync(body, { level: 9 });
    }
    files.set(url, entry);
  }
  if (files.size === 0) throw new Error(`${folder} holds no files: build the React pages first`);

  const server = createServer((request, response) => {
    let path;
    try {
      path = posix.normalize(decodeURIComponent(new URL(request.url, "http://host").pathname));
    } catch {
      response.writeHead(400).end();
      return;
    }
    if (path === "/favicon.ico") {
      response.writeHead(204).end();
      return;
    }
    const entry = files.get(path);
    if (!entry || (request.method !== "GET" && request.method !== "HEAD")) {
      response.writeHead(entry ? 405 : 404, { "content-type": "text/plain; charset=utf-8" }).end(entry ? "" : "Not found");
      return;
    }
    const accepts = String(request.headers["accept-encoding"] || "");
    const headers = {
      "content-type": entry.type,
      vary: "Accept-Encoding",
      "x-content-type-options": "nosniff",
      "cache-control": path.startsWith("/assets/") ? "public, max-age=31536000, immutable" : "no-cache",
    };
    let body = entry.body;
    if (entry.br && /\bbr\b/.test(accepts)) {
      headers["content-encoding"] = "br";
      body = entry.br;
    } else if (entry.gzip && /\bgzip\b/.test(accepts)) {
      headers["content-encoding"] = "gzip";
      body = entry.gzip;
    }
    headers["content-length"] = body.length;
    response.writeHead(200, headers);
    response.end(request.method === "HEAD" ? undefined : body);
  });
  server.keepAliveTimeout = 30000;
  await new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const { port } = server.address();
  return {
    url: `http://127.0.0.1:${port}`,
    stop: () => new Promise((done) => {
      server.closeAllConnections?.();
      server.close(() => done());
    }),
  };
}

// ------------------------------------------------------------ the React build

/** Installs the React project's packages if they are missing, then builds its pages. */
export function buildReact(folder, { log, skipBuild = false }) {
  const npm = isWindows ? "npm.cmd" : "npm";
  const vite = join(folder, "node_modules", "vite", "bin", "vite.js");
  if (!existsSync(vite)) {
    const locked = existsSync(join(folder, "package-lock.json"));
    log(`Installing the React project's packages (${locked ? "npm ci" : "npm install: there is no package-lock.json yet"}) ...`);
    run(npm, [locked ? "ci" : "install"], { cwd: folder, shell: isWindows, env: { npm_config_audit: "false", npm_config_fund: "false" } });
  }
  if (skipBuild && existsSync(join(folder, "dist", "table.html"))) {
    log("Using the React pages already built in react/dist.");
    return;
  }
  log("Building the React pages (vite build) ...");
  run(process.execPath, [vite, "build"], { cwd: folder });
  for (const page of ["table", "select", "dates", "tabs"]) {
    if (!existsSync(join(folder, "dist", `${page}.html`))) throw new Error(`vite build did not make dist/${page}.html`);
  }
}
