/**
 * Serves the static export in `out/` the way a static host does:
 * `/docs/x/` → `out/docs/x/index.html`, unknown paths → `out/404.html`.
 *
 * Text files (HTML, scripts, styles, JSON, text, Markdown, SVG) are sent
 * compressed with Brotli, or gzip when the client does not accept Brotli,
 * as the live site's CDN sends them. The scores that Lighthouse gives
 * (`scripts/lighthouse.ts`) depend on the bytes sent, so a server that does
 * not compress would measure the site 15 to 20 points low. `COMPRESS=0`
 * turns it off.
 *
 *   pnpm build && pnpm serve            # http://127.0.0.1:3211
 *   PORT=4000 pnpm serve
 *   DEMO_UI=http://127.0.0.1:8095 pnpm serve    # with the live theme demo (`cargo run -p theme-demo`)
 */
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

const OUT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "out");
const PORT = Number(process.env.PORT ?? 3211);
const HOST = process.env.HOST ?? "127.0.0.1";
const COMPRESS = process.env.COMPRESS !== "0";

/** The file types worth compressing; images and fonts are compressed already. */
const COMPRESSIBLE = new Set([".html", ".js", ".css", ".json", ".txt", ".md", ".svg", ".xml", ".webmanifest"]);
const MIN_COMPRESS_BYTES = 256;

/** Compressed bodies by encoding, file and modification time, so each file is compressed once. */
const compressed = new Map<string, Buffer>();

/** Brotli if the client accepts it, else gzip, else nothing. */
function encodingFor(acceptEncoding: string | string[] | undefined): "br" | "gzip" | null {
  const accepted = String(acceptEncoding ?? "").toLowerCase();
  if (/\bbr\b/.test(accepted)) return "br";
  if (/\bgzip\b/.test(accepted)) return "gzip";
  return null;
}

function compress(file: string, encoding: "br" | "gzip"): Buffer {
  const key = `${encoding}:${file}:${fs.statSync(file).mtimeMs}`;
  let body = compressed.get(key);
  if (!body) {
    const raw = fs.readFileSync(file);
    body =
      encoding === "br"
        ? zlib.brotliCompressSync(raw, { params: { [zlib.constants.BROTLI_PARAM_QUALITY]: 5, [zlib.constants.BROTLI_PARAM_SIZE_HINT]: raw.length } })
        : zlib.gzipSync(raw, { level: 6 });
    compressed.set(key, body);
  }
  return body;
}

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

/**
 * With `DEMO_UI` (the live theme demo's address, `http://127.0.0.1:8095`), the
 * demo's pages and the toolkit's files and routes are passed to it, as the live
 * site's reverse proxy passes them: `/demo/ui/` and `/_navree/ui/`.
 */
const DEMO_UI = process.env.DEMO_UI ? new URL(process.env.DEMO_UI) : null;
const DEMO_PATHS = ["/demo/ui/", "/demo/ui", "/_navree/ui/"];

function toDemo(req: http.IncomingMessage, res: http.ServerResponse, demo: URL) {
  const upstream = http.request(
    {
      hostname: demo.hostname,
      port: demo.port,
      method: req.method,
      path: req.url,
      headers: { ...req.headers, "x-forwarded-for": req.socket.remoteAddress ?? "127.0.0.1", "x-forwarded-proto": "http" },
    },
    (answer) => {
      res.writeHead(answer.statusCode ?? 502, answer.headers);
      answer.pipe(res);
    },
  );
  upstream.on("error", () => {
    if (!res.headersSent) res.writeHead(502, { "Content-Type": "text/plain; charset=utf-8" });
    res.end("The theme demo isn't answering.");
  });
  req.pipe(upstream);
}

http
  .createServer((req, res) => {
    const url = req.url ?? "/";
    const pathname = url.split("?")[0];
    if (DEMO_UI && DEMO_PATHS.some((prefix) => pathname === prefix || pathname.startsWith(prefix))) {
      toDemo(req, res, DEMO_UI);
      return;
    }
    // Directory URLs without the trailing slash redirect, like most hosts.
    if (!pathname.endsWith("/") && !path.extname(pathname) && fs.existsSync(path.join(OUT, pathname, "index.html"))) {
      res.writeHead(308, { Location: `${pathname}/` });
      res.end();
      return;
    }
    const file = resolve(url);
    const status = file ? 200 : 404;
    const body = file ?? path.join(OUT, "404.html");
    const extension = path.extname(body);
    const headers: http.OutgoingHttpHeaders = { "Content-Type": types[extension] ?? "application/octet-stream" };
    const encoding = COMPRESS && COMPRESSIBLE.has(extension) && fs.statSync(body).size >= MIN_COMPRESS_BYTES ? encodingFor(req.headers["accept-encoding"]) : null;
    if (COMPRESS && COMPRESSIBLE.has(extension)) headers.Vary = "Accept-Encoding";
    if (encoding) {
      const bytes = compress(body, encoding);
      res.writeHead(status, { ...headers, "Content-Encoding": encoding, "Content-Length": bytes.length });
      res.end(req.method === "HEAD" ? undefined : bytes);
      return;
    }
    res.writeHead(status, headers);
    fs.createReadStream(body).pipe(res);
  })
  .listen(PORT, HOST, () => {
    console.log(`Serving out/ at http://${HOST}:${PORT}`);
  });
