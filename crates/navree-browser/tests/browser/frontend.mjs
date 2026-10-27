// A small web front end with a BFF, for navree-browser's FrontEnd tests:
// `node frontend.mjs <port>`. Its pages are rendered on the server from the
// API at FAKE_API_URL; /api/* is forwarded to it.
import http from "node:http";

const port = Number(process.argv[2]);
const api = process.env.FAKE_API_URL;
const greeting = process.env.FAKE_GREETING ?? "no greeting";

const server = http.createServer(async (req, res) => {
  try {
    if (req.url.startsWith("/api/")) {
      const upstream = await fetch(api + req.url, { method: req.method });
      res.writeHead(upstream.status, { "content-type": upstream.headers.get("content-type") ?? "text/plain" });
      res.end(Buffer.from(await upstream.arrayBuffer()));
      return;
    }
    if (req.url === "/") {
      const body = await (await fetch(`${api}/api/label`)).json();
      res.writeHead(200, { "content-type": "text/html" });
      res.end(
        `<!doctype html><title>Front end</title><h1>Front end</h1>` +
          `<p id="label">Serving ${body.label}</p><p id="pid">pid ${process.pid}</p><p>${greeting}</p>`,
      );
      return;
    }
    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  } catch (error) {
    res.writeHead(502, { "content-type": "text/plain" });
    res.end(`the API failed: ${error}`);
  }
});

server.listen(port, "127.0.0.1");
