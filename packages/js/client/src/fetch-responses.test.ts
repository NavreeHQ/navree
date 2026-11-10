import { createServer, type IncomingMessage, type Server } from "node:http";
import type { AddressInfo, Socket } from "node:net";
import { afterEach, describe, expect, it } from "vitest";
import { BASE, json, recording } from "../test/fetch-helpers.js";
import type { paths } from "../test/fixtures/typed.js";
import { createClient } from "./fetch.js";

describe("responses", () => {
  it("parses a JSON body into data and returns the response", async () => {
    const { client } = recording(() =>
      json([{ id: "1", name: "Rex" }], { headers: { "content-type": "application/json", "content-length": "26" } }),
    );
    const result = await client.GET("/pets");
    expect(result.data).toEqual([{ id: "1", name: "Rex" }]);
    expect(result.error).toBeUndefined();
    expect(result.response.status).toBe(200);
  });

  it("parses JSON whatever the content type, when there is no Content-Length", async () => {
    const { client } = recording(() => new Response('{"id":"1","name":"Rex"}', { headers: { "content-type": "text/plain" } }));
    const { data } = await client.GET("/pets/{petId}", { params: { path: { petId: "1" } } });
    expect(data).toEqual({ id: "1", name: "Rex" });
  });

  it("gives data undefined for a 200 with an empty body", async () => {
    const { client } = recording(() => new Response("", { status: 200 }));
    const result = await client.GET("/pets");
    expect(result.data).toBeUndefined();
    expect(result.error).toBeUndefined();
  });

  it("gives data undefined for 204, and error undefined for an empty error", async () => {
    const { client } = recording((request) =>
      request.method === "DELETE"
        ? new Response(null, { status: 204 })
        : new Response(null, { status: 500, headers: { "content-length": "0" } }),
    );
    const deleted = await client.DELETE("/pets/{petId}", { params: { path: { petId: "1" } } });
    expect(deleted.data).toBeUndefined();
    expect(deleted.error).toBeUndefined();
    expect(deleted.response.status).toBe(204);

    const failed = await client.GET("/pets");
    expect(failed.data).toBeUndefined();
    expect(failed.error).toBeUndefined();
    expect(failed.response.status).toBe(500);
  });

  it("gives data undefined for Content-Length: 0 unless the body is chunked", async () => {
    const { client } = recording(() => new Response("", { headers: { "content-length": "0" } }));
    expect((await client.GET("/pets")).data).toBeUndefined();

    const chunked = recording(
      () => new Response('{"id":"1","name":"Rex"}', { headers: { "content-length": "0", "transfer-encoding": "chunked" } }),
    );
    expect((await chunked.client.GET("/pets/{petId}", { params: { path: { petId: "1" } } })).data).toEqual({
      id: "1",
      name: "Rex",
    });
  });

  it("reads nothing for HEAD", async () => {
    const { client } = recording(() => new Response("ignored", { headers: { "content-length": "7" } }));
    const result = await client.HEAD("/files/{name}", { params: { path: { name: "a" } } });
    expect(result.data).toBeUndefined();
    expect(result.response.status).toBe(200);
  });

  it("rejects a malformed JSON body of a success", async () => {
    const { client } = recording(() => new Response("{not json", { headers: { "content-length": "9" } }));
    await expect(client.GET("/pets")).rejects.toBeInstanceOf(SyntaxError);
    const noLength = recording(() => new Response("{not json"));
    await expect(noLength.client.GET("/pets")).rejects.toBeInstanceOf(SyntaxError);
  });

  it("parses errors as JSON when they are, and keeps the text when they are not", async () => {
    const problem = { code: "not_found", message: "No such pet" };
    const { client } = recording((request) =>
      request.url.endsWith("/pets/1") ? json(problem, { status: 404 }) : new Response("upstream down", { status: 502 }),
    );
    const notFound = await client.GET("/pets/{petId}", { params: { path: { petId: "1" } } });
    expect(notFound.data).toBeUndefined();
    expect(notFound.error).toEqual(problem);
    expect(notFound.response.status).toBe(404);

    const down = await client.GET("/pets/{petId}", { params: { path: { petId: "2" } } });
    expect(down.error).toBe("upstream down");

    const empty = recording(() => new Response("", { status: 500 }));
    expect((await empty.client.GET("/pets")).error).toBe("");
  });

  it("parses a null or numeric JSON error body as that value", async () => {
    const { client } = recording(() => new Response("null", { status: 500 }));
    expect((await client.GET("/pets")).error).toBeNull();
    const text = recording(() => new Response("123", { status: 500 }));
    expect((await text.client.GET("/pets")).error).toBe(123);
  });

  it("parses as text, blob, arrayBuffer or stream when asked", async () => {
    const { client } = recording(
      () => new Response("hello", { headers: { "content-type": "application/octet-stream", "content-length": "5" } }),
    );
    const name = { params: { path: { name: "a" } } };

    const text = await client.GET("/files/{name}", { ...name, parseAs: "text" });
    expect(text.data).toBe("hello");

    const blob = await client.GET("/files/{name}", { ...name, parseAs: "blob" });
    expect(await (blob.data as unknown as Blob).text()).toBe("hello");

    const buffer = await client.GET("/files/{name}", { ...name, parseAs: "arrayBuffer" });
    expect(new TextDecoder().decode(buffer.data as unknown as ArrayBuffer)).toBe("hello");

    const stream = await client.GET("/files/{name}", { ...name, parseAs: "stream" });
    expect(await new Response(stream.data as unknown as ReadableStream).text()).toBe("hello");
  });

  it("parses as text without a Content-Length, and ignores parseAs for an error", async () => {
    const { client } = recording((request) =>
      request.method === "GET" ? new Response("plain") : new Response("bad", { status: 400 }),
    );
    const name = { params: { path: { name: "a" } } };
    expect((await client.GET("/files/{name}", { ...name, parseAs: "text" })).data).toBe("plain");
    const failed = await client.PUT("/files/{name}", { ...name, body: new FormData() as never, parseAs: "blob" });
    expect(failed.error).toBe("bad");
  });
});

describe("middleware", () => {
  it("runs onRequest in order, then onResponse in reverse, the client's before the request's", async () => {
    const calls: string[] = [];
    const { client } = recording(() => json([]));
    const mw = (name: string) => ({
      onRequest: () => void calls.push(`request ${name}`),
      onResponse: () => void calls.push(`response ${name}`),
    });
    client.use(mw("a"), mw("b"));
    await client.GET("/pets", { middleware: [mw("r")] });
    expect(calls).toEqual(["request a", "request b", "request r", "response r", "response b", "response a"]);
  });

  it("lets onRequest replace the request, or answer without calling fetch", async () => {
    const fetched: string[] = [];
    const { client } = recording((request) => {
      fetched.push(request.headers.get("x-added") ?? "none");
      return json([]);
    });
    client.use({
      onRequest: ({ request }) => {
        if (request.url.endsWith("/search?q=cached")) return json({ hits: [] });
        return new Request(request, { headers: { "x-added": "yes" } });
      },
    });
    await client.GET("/pets");
    expect(fetched).toEqual(["yes"]);

    const cached = await client.GET("/search", { params: { query: { q: "cached" } } });
    expect(fetched).toEqual(["yes"]);
    expect(cached.data).toEqual({ hits: [] });
  });

  it("skips the response hooks of an answer an onRequest gave", async () => {
    const calls: string[] = [];
    const { client } = recording(() => json([]));
    client.use({ onResponse: () => void calls.push("response") });
    client.use({ onRequest: () => json([{ id: "1", name: "Cached" }]) });
    expect((await client.GET("/pets")).data).toEqual([{ id: "1", name: "Cached" }]);
    expect(calls).toEqual([]);
  });

  it("lets onResponse replace the response", async () => {
    const { client } = recording(() => json([]));
    client.use({ onResponse: () => json([{ id: "9", name: "Swapped" }]) });
    expect((await client.GET("/pets")).data).toEqual([{ id: "9", name: "Swapped" }]);
  });

  it("rejects an onRequest or onResponse that returns something else", async () => {
    const { client } = recording(() => json([]));
    const bad = { onRequest: () => ({}) as never };
    client.use(bad);
    await expect(client.GET("/pets")).rejects.toThrow("onRequest: must return new Request() or Response()");
    client.eject(bad);
    client.use({ onResponse: () => ({}) as never });
    await expect(client.GET("/pets")).rejects.toThrow("onResponse: must return new Response()");
  });

  it("lets onError turn a failure into a response or a different error, or let it through", async () => {
    const failing = () => {
      throw new TypeError("offline");
    };
    const handled = recording(failing);
    handled.client.use({ onError: () => json([{ id: "1", name: "Recovered" }]) });
    expect((await handled.client.GET("/pets")).data).toEqual([{ id: "1", name: "Recovered" }]);

    const swapped = recording(failing);
    swapped.client.use({ onError: ({ error }) => new Error(`wrapped: ${(error as Error).message}`) });
    await expect(swapped.client.GET("/pets")).rejects.toThrow("wrapped: offline");

    const untouched = recording(failing);
    untouched.client.use({ onError: () => undefined });
    await expect(untouched.client.GET("/pets")).rejects.toThrow("offline");

    const invalid = recording(failing);
    invalid.client.use({ onError: () => ({}) as never });
    await expect(invalid.client.GET("/pets")).rejects.toThrow("onError: must return new Response() or instance of Error");
  });

  it("runs onError hooks in reverse order, each seeing the error the one before it left", async () => {
    const seen: string[] = [];
    const { client } = recording(() => {
      throw new Error("e0");
    });
    client.use({
      onError: ({ error }) => {
        seen.push(`first saw ${(error as Error).message}`);
        return new Error("e-first");
      },
    });
    client.use({
      onError: ({ error }) => {
        seen.push(`second saw ${(error as Error).message}`);
        return new Error("e-second");
      },
    });
    await expect(client.GET("/pets")).rejects.toThrow("e-first");
    expect(seen).toEqual(["second saw e0", "first saw e-second"]);
  });

  it("hands each hook the request, schema path, params, options and one id per request", async () => {
    const seen: Array<{ schemaPath: string; params: unknown; id: string; frozen: boolean; parseAs: string; baseUrl: string }> = [];
    const { client } = recording(() => json([]));
    client.use({
      onRequest: ({ schemaPath, params, id, options }) => {
        seen.push({ schemaPath, params, id, frozen: Object.isFrozen(options), parseAs: options.parseAs, baseUrl: options.baseUrl });
      },
      onResponse: ({ id, response }) => {
        expect(id).toBe(seen[seen.length - 1]!.id);
        expect(response.status).toBe(200);
      },
    });
    await client.GET("/search", { params: { query: { q: "x" } } });
    await client.GET("/pets");
    expect(seen[0]).toMatchObject({ schemaPath: "/search", params: { query: { q: "x" } }, frozen: true, parseAs: "json", baseUrl: BASE });
    expect(seen[1]!.params).toEqual({});
    expect(seen[0]!.id).not.toBe(seen[1]!.id);
    expect(seen[0]!.id).toMatch(/^[a-z0-9]{1,9}$/);
  });

  it("ejects middleware, and refuses a middleware with no hook", async () => {
    const calls: string[] = [];
    const { client } = recording(() => json([]));
    const mw = { onRequest: () => void calls.push("hit") };
    client.use(mw);
    await client.GET("/pets");
    client.eject(mw);
    await client.GET("/pets");
    expect(calls).toEqual(["hit"]);

    expect(() => client.use({} as never)).toThrow("Middleware must be an object with one of");
    expect(() => client.use("x" as never)).toThrow("Middleware must be an object with one of");
    expect(() => client.use(undefined as never)).not.toThrow();
    expect(() => client.eject({ onRequest: () => undefined })).not.toThrow();
  });
});

describe("against a real server", () => {
  let server: Server | undefined;
  const sockets = new Set<Socket>();

  afterEach(async () => {
    for (const socket of sockets) socket.destroy();
    await new Promise<void>((done) => (server ? server.close(() => done()) : done()));
    server = undefined;
  });

  async function listen(
    handler: (req: IncomingMessage) => { status?: number; headers?: Record<string, string>; body?: string; hang?: boolean },
  ) {
    const seen: Array<{ method: string; url: string; headers: IncomingMessage["headers"]; body: string }> = [];
    server = createServer((req, res) => {
      const chunks: Buffer[] = [];
      req.on("data", (chunk: Buffer) => chunks.push(chunk));
      req.on("end", () => {
        seen.push({ method: req.method ?? "", url: req.url ?? "", headers: req.headers, body: Buffer.concat(chunks).toString() });
        const out = handler(req);
        if (out.hang) return;
        res.writeHead(out.status ?? 200, out.headers ?? { "content-type": "application/json" });
        res.end(out.body ?? "{}");
      });
    });
    server.on("connection", (socket) => {
      sockets.add(socket);
      socket.on("close", () => sockets.delete(socket));
    });
    await new Promise<void>((done) => server!.listen(0, "127.0.0.1", done));
    const { port } = server.address() as AddressInfo;
    return { base: `http://127.0.0.1:${port}`, seen };
  }

  it("sends a typed request over the wire and parses the answer", async () => {
    const { base, seen } = await listen(() => ({ status: 201, body: '{"id":"7","name":"Rex","tag":"dog"}' }));
    const client = createClient<paths>({ baseUrl: base, headers: { "X-Client": "1" } });
    const { data, response } = await client.POST("/pets", { body: { name: "Rex", tag: "dog" }, headers: { "X-Call": "2" } });

    expect(response.status).toBe(201);
    expect(data).toEqual({ id: "7", name: "Rex", tag: "dog" });
    expect(seen[0]).toMatchObject({ method: "POST", url: "/pets", body: '{"name":"Rex","tag":"dog"}' });
    expect(seen[0]!.headers["content-type"]).toBe("application/json");
    expect(seen[0]!.headers["x-client"]).toBe("1");
    expect(seen[0]!.headers["x-call"]).toBe("2");
  });

  it("encodes path and query the way the server sees them", async () => {
    const { base, seen } = await listen(() => ({ body: "[]" }));
    const client = createClient<paths>({ baseUrl: base });
    await client.GET("/pets", { params: { query: { limit: 10, tags: ["a b", "c&d"], filter: { name: "Rex" } } } });
    await client.GET("/pets/{petId}", { params: { path: { petId: "a/b c" } } });
    expect(seen[0]!.url).toBe("/pets?limit=10&tags=a%20b&tags=c%26d&filter[name]=Rex");
    expect(seen[1]!.url).toBe("/pets/a%2Fb%20c");
  });

  it("reads a body that has a Content-Length, and a chunked one", async () => {
    const { base } = await listen((req) =>
      req.url === "/pets/1"
        ? { body: '{"id":"1","name":"Rex"}' }
        : { headers: { "content-type": "application/json", "transfer-encoding": "chunked" }, body: '{"id":"2","name":"Fido"}' },
    );
    const client = createClient<paths>({ baseUrl: base });
    expect((await client.GET("/pets/{petId}", { params: { path: { petId: "1" } } })).data).toEqual({ id: "1", name: "Rex" });
    expect((await client.GET("/pets/{petId}", { params: { path: { petId: "2" } } })).data).toEqual({ id: "2", name: "Fido" });
  });

  it("answers 204 with no data and an error status with the parsed error", async () => {
    const { base } = await listen((req) =>
      req.method === "DELETE"
        ? { status: 204, headers: {}, body: "" }
        : { status: 404, body: '{"code":"not_found","message":"gone"}' },
    );
    const client = createClient<paths>({ baseUrl: base });
    const deleted = await client.DELETE("/pets/{petId}", { params: { path: { petId: "1" } } });
    expect(deleted.response.status).toBe(204);
    expect(deleted.data).toBeUndefined();
    const missing = await client.GET("/pets/{petId}", { params: { path: { petId: "1" } } });
    expect(missing.error).toEqual({ code: "not_found", message: "gone" });
  });

  it("aborts a request that the server holds open", async () => {
    const { base } = await listen(() => ({ hang: true }));
    const client = createClient<paths>({ baseUrl: base });
    const controller = new AbortController();
    const pending = client.GET("/pets", { signal: controller.signal });
    setTimeout(() => controller.abort(), 20);
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });

  it("aborts on a timeout signal", async () => {
    const { base } = await listen(() => ({ hang: true }));
    const client = createClient<paths>({ baseUrl: base });
    await expect(client.GET("/pets", { signal: AbortSignal.timeout(30) })).rejects.toMatchObject({ name: "TimeoutError" });
  });

  it("sends form bodies and multipart bodies over the wire", async () => {
    const { base, seen } = await listen(() => ({ status: 201, body: '{"id":"1","name":"a"}' }));
    const client = createClient<paths>({ baseUrl: base });
    await client.POST("/pets", { body: { name: "a b" }, headers: { "content-type": "application/x-www-form-urlencoded" } });
    expect(seen[0]!.body).toBe("name=a+b");

    const form = new FormData();
    form.set("note", "hi");
    await client.PUT("/files/{name}", { params: { path: { name: "n" } }, body: form as never });
    expect(seen[1]!.headers["content-type"]).toMatch(/^multipart\/form-data; boundary=/);
    expect(seen[1]!.body).toContain('name="note"');
  });
});
