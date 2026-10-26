/**
 * Serves the static export in `out/` the way a static host does:
 * `/docs/x/` → `out/docs/x/index.html`, unknown paths → `out/404.html`.
 *
 *   pnpm build && pnpm serve            # http://127.0.0.1:3211
 *   PORT=4000 pnpm serve
 */
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OUT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "out");
const PORT = Number(process.env.PORT ?? 3211);
const HOST = process.env.HOST ?? "127.0.0.1";

const types: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".md": "text/markdown; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".webmanifest": "application/manifest+json",
};

function resolve(urlPath: string): string | null {
  const clean = decodeURIComponent(urlPath.split("?")[0]);
  const target = path.join(OUT, clean);
  if (!target.startsWith(OUT)) return null;
  const candidates = [target, path.join(target, "index.html"), `${target}.html`];
  return candidates.find((file) => fs.existsSync(file) && fs.statSync(file).isFile()) ?? null;
}

if (!fs.existsSync(OUT)) {
  console.error("out/ does not exist: run `pnpm build` first.");
  process.exit(1);
}

http
  .createServer((req, res) => {
    const url = req.url ?? "/";
    const pathname = url.split("?")[0];
    // Directory URLs without the trailing slash redirect, like most hosts.
    if (!pathname.endsWith("/") && !path.extname(pathname) && fs.existsSync(path.join(OUT, pathname, "index.html"))) {
      res.writeHead(308, { Location: `${pathname}/` });
      res.end();
      return;
    }
    const file = resolve(url);
    const status = file ? 200 : 404;
    const body = file ?? path.join(OUT, "404.html");
    res.writeHead(status, { "Content-Type": types[path.extname(body)] ?? "application/octet-stream" });
    fs.createReadStream(body).pipe(res);
  })
  .listen(PORT, HOST, () => {
    console.log(`Serving out/ at http://${HOST}:${PORT}`);
  });
