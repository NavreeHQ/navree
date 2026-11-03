// A hostile server for the runtime: the committed dist/ under the page's exact
// content security policy, with replies a test chooses, and a second origin that
// records everything it receives.

import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const crate = join(dirname(fileURLToPath(import.meta.url)), "../../../../crates/navree-ui");
const file = (name) => readFileSync(join(crate, "dist", name));
const FILES = { "/_navree/ui/core.js": file("core.js"), "/_navree/ui/overlay.js": file("overlay.js"), "/_navree/ui/menus.js": file("menus.js"), "/_navree/ui/navree-ui.css": file("navree-ui.css") };

/** The policy the crate sends, read from its source. */
export const CSP = /PAGE_CSP: &str = "([^"]+)"/.exec(readFileSync(join(crate, "src/view.rs"), "utf8"))[1];
export const BUILD = "b1";

const MARKER = (name) => `document.documentElement.setAttribute("${name}", "1");`;

/** A page with the core runtime, as `View` writes one; `menus` adds the menus module, `css` the stylesheet. */
export function page(body, { title = "t", build = BUILD, head = "", menus = false, css = false } = {}) {
  const module = menus ? `<script type="module" src="/_navree/ui/menus.js"></script>` : "";
  const sheet = css ? `<link rel="stylesheet" href="/_navree/ui/navree-ui.css">` : "";
  return `<!doctype html><html lang="en" data-build="${build}"><head><meta charset="utf-8"><title>${title}</title>${sheet}<script type="module" src="/_navree/ui/core.js"></script>${module}${head}</head><body data-ui-boost>${body}</body></html>`;
}

export async function start() {
  const log = [];
  const evil = [];
  /** path -> (req, res, body, url) => void */
  const routes = new Map();
  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      const body = Buffer.concat(chunks).toString();
      log.push({ method: req.method, path: url.pathname + url.search, headers: req.headers, body });
      if (FILES[url.pathname]) {
        res.writeHead(200, { "content-type": url.pathname.endsWith(".css") ? "text/css" : "text/javascript" });
        return res.end(FILES[url.pathname]);
      }
      if (url.pathname === "/attacker.js" || url.pathname === "/_navree/ui/stand-in.js") {
        res.writeHead(200, { "content-type": "text/javascript" });
        return res.end(MARKER(url.pathname === "/attacker.js" ? "data-attacker-ran" : "data-stand-in-ran"));
      }
      const route = routes.get(url.pathname);
      if (!route) {
        res.writeHead(404, { "content-type": "text/plain" });
        return res.end("nope");
      }
      route(req, res, body, url);
    });
  });
  const evilServer = createServer((req, res) => {
    evil.push({ method: req.method, url: req.url, headers: req.headers });
    res.writeHead(200, { "content-type": "text/html", "access-control-allow-origin": "*" });
    res.end("<!doctype html><title>EVIL</title><h1>evil origin</h1>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  await new Promise((resolve) => evilServer.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const evilOrigin = `http://127.0.0.1:${evilServer.address().port}`;
  const html = (res, text, headers = {}) => {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8", "content-security-policy": CSP, "cache-control": "no-store", ...headers });
    res.end(text);
  };
  const ops = (res, envelope, status = 200, headers = {}) => {
    res.writeHead(status, { "content-type": "application/vnd.navree.ops+json", "cache-control": "no-store", ...headers });
    res.end(JSON.stringify(envelope));
  };
  return { log, evil, routes, origin, evilOrigin, html, ops, close: () => { server.close(); evilServer.close(); } };
}
