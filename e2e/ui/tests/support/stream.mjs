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
  "/_navree/ui/live.js": ["text/javascript", dist("live.js")],
  "/_navree/ui/lazy.js": ["text/javascript", dist("lazy.js")],
  "/_navree/ui/navree-ui.css": ["text/css", dist("navree-ui.css")],
};
export const CSP = /PAGE_CSP: &str = "([^"]+)"/.exec(readFileSync(join(crate, "src/view.rs"), "utf8"))[1];

/** A page with the core runtime and the live module, as `View` writes one for a live region; `lazy` adds the lazy list's module. */
export function page(body, { css = false, lazy = false } = {}) {
  return `<!doctype html><html lang="en" data-build="b1"><head><meta charset="utf-8"><title>t</title>${css ? '<link rel="stylesheet" href="/_navree/ui/navree-ui.css">' : ""}<script type="module" src="/_navree/ui/core.js"></script><script type="module" src="/_navree/ui/live.js"></script>${lazy ? '<script type="module" src="/_navree/ui/lazy.js"></script>' : ""}</head><body data-ui-boost><main>${body}</main></body></html>`;
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Resolves when `check()` is true. A test waits for what it expects to happen, not for a
 * time: `limit` only ends a hang, so it is long, and a slow machine takes as long as it takes.
 */
export async function until(check, what, limit = 60000) {
  const end = Date.now() + limit;
  while (!(await check())) {
    if (Date.now() > end) throw new Error(`gave up waiting for ${what} after ${limit} ms`);
    await sleep(25);
  }
}

/** The times between one moment and the next. */
export const gaps = (times) => times.slice(1).map((t, i) => t - times[i]);

/**
 * Starts a server. `log` has every request; `routes` maps a path to `(req, res, url) => void`;
 * `stream(talk)` makes a route that opens an event stream; `say(text)` writes to every open one.
 * A probe the runtime makes (`x-ui-probe`) is logged but not counted as a stream attempt.
 * `sse.times` has the moment each attempt arrived.
 */
export async function start() {
  const log = [];
  const routes = new Map();
  const sse = { attempts: 0, times: [], open: 0, cookies: [], clients: new Set() };
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
  /**
   * An answer that stands for one of the app's own pages, or a region's answer: it carries `x-navree-page`, as every
   * answer of the app's `View` does, and the runtime morphs only an answer that has it. A header given as `null` is
   * left out, so a test that means "this is no page of the app" passes `{ "x-navree-page": null }`.
   */
  const html = (res, text, headers = {}) => {
    const sent = { "content-type": "text/html; charset=utf-8", "content-security-policy": CSP, "cache-control": "no-store", "x-navree-page": "1", ...headers };
    for (const name of Object.keys(sent)) if (sent[name] === null) delete sent[name];
    res.writeHead(200, sent);
    res.end(text);
  };
  const stream = (talk) => (req, res) => {
    if (!("x-ui-probe" in req.headers)) {
      sse.attempts++;
      sse.times.push(Date.now());
    }
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
