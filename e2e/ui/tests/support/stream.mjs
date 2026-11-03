// A server for live regions: the committed dist/ runtime under the crate's exact content
// security policy, with a stream and fragments that each test controls.

import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const crate = join(dirname(fileURLToPath(import.meta.url)), "../../../../crates/navree-ui");
const dist = (name) => readFileSync(join(crate, "dist", name));
const FILES = {
  "/_navree/ui/core.js": ["text/javascript", dist("core.js")],
  "/_navree/ui/table.js": ["text/javascript", dist("table.js")],
  "/_navree/ui/navree-ui.css": ["text/css", dist("navree-ui.css")],
};
export const CSP = /PAGE_CSP: &str = "([^"]+)"/.exec(readFileSync(join(crate, "src/view.rs"), "utf8"))[1];

/** A page with the core runtime, as `View` writes one. */
export function page(body, { css = false } = {}) {
  return `<!doctype html><html lang="en" data-build="b1"><head><meta charset="utf-8"><title>t</title>${css ? '<link rel="stylesheet" href="/_navree/ui/navree-ui.css">' : ""}<script type="module" src="/_navree/ui/core.js"></script></head><body data-ui-boost><main>${body}</main></body></html>`;
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Starts a server. `log` has every request; `routes` maps a path to `(req, res, url) => void`;
 * `stream(talk)` makes a route that opens an event stream; `say(text)` writes to every open one.
 * A probe the runtime makes (`x-ui-probe`) is logged but not counted as a stream attempt.
 */
export async function start() {
  const log = [];
  const routes = new Map();
  const sse = { attempts: 0, open: 0, cookies: [], clients: new Set() };
  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    log.push({ t: Date.now(), method: req.method, path: url.pathname + url.search, cookie: req.headers.cookie || "", probe: "x-ui-probe" in req.headers });
    if (FILES[url.pathname]) {
      res.writeHead(200, { "content-type": FILES[url.pathname][0] });
      return res.end(FILES[url.pathname][1]);
    }
    const route = routes.get(url.pathname);
    if (!route) {
      res.writeHead(404, { "content-type": "text/plain" });
      return res.end("nope");
    }
    route(req, res, url);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const html = (res, text, headers = {}) => {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8", "content-security-policy": CSP, "cache-control": "no-store", ...headers });
    res.end(text);
  };
  const stream = (talk) => (req, res) => {
    if (!("x-ui-probe" in req.headers)) sse.attempts++;
    sse.cookies.push(req.headers.cookie || "");
    res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store" });
    res.write(": ready\n\n");
    sse.open++;
    sse.clients.add(res);
    req.on("close", () => {
      sse.open--;
      sse.clients.delete(res);
    });
    talk?.(res, sse.attempts);
  };
  const say = (text) => {
    for (const res of sse.clients) res.write(text);
  };
  /** A fragment route that answers after `ms`, counting how many are in flight at once. */
  const gauge = { inflight: 0, most: 0, asked: 0 };
  const slow = (ms, answer) => (req, res, url) => {
    gauge.asked++;
    gauge.inflight++;
    gauge.most = Math.max(gauge.most, gauge.inflight);
    res.on("close", () => gauge.inflight--);
    setTimeout(() => answer(res, url, gauge.asked), ms);
  };
  return {
    log,
    routes,
    origin,
    html,
    stream,
    say,
    sse,
    gauge,
    slow,
    count: (path) => log.filter((l) => l.path.startsWith(path) && !l.probe).length,
    close: () => {
      for (const r of sse.clients) r.destroy();
      server.close();
    },
  };
}
