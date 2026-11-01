import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { BASE, json, recording } from "../test/fetch-helpers.js";
import type { paths } from "../test/fixtures/typed.js";
import {
  createClient,
  createFinalURL,
  createQuerySerializer,
  defaultPathSerializer,
  mergeHeaders,
  serializeArrayParam,
  serializeObjectParam,
  serializePrimitiveParam,
} from "./fetch.js";

describe("urls", () => {
  it("joins the base URL and the path, trimming one trailing slash from the base", async () => {
    const { client, requests } = recording(undefined, { baseUrl: `${BASE}/v1/` });
    await client.GET("/pets");
    expect(requests[0]!.url).toBe(`${BASE}/v1/pets`);
  });

  it("takes a base URL for one request", async () => {
    const { client, requests } = recording();
    await client.GET("/pets", { baseUrl: "http://other.test/" });
    expect(requests[0]!.url).toBe("http://other.test/pets");
  });

  it("percent-encodes path parameters", async () => {
    const { client, requests } = recording();
    await client.GET("/pets/{petId}", { params: { path: { petId: "a b/c?d#e" } } });
    expect(requests[0]!.url).toBe(`${BASE}/pets/a%20b%2Fc%3Fd%23e`);
  });

  it("leaves a placeholder alone when its parameter is missing", async () => {
    const { client, requests } = recording();
    await client.GET("/pets/{petId}", { params: { path: {} as { petId: string } } });
    expect(requests[0]!.url).toBe(`${BASE}/pets/%7BpetId%7D`);
  });

  it("appends the query after the path", async () => {
    const { client, requests } = recording();
    await client.GET("/search", { params: { query: { q: "fluffy cat", page: 2 } } });
    expect(requests[0]!.url).toBe(`${BASE}/search?q=fluffy%20cat&page=2`);
  });

  it("adds no question mark when the query is empty", async () => {
    const { client, requests } = recording();
    await client.GET("/pets", { params: { query: {} } });
    await client.GET("/pets");
    expect(requests.map((r) => r.url)).toEqual([`${BASE}/pets`, `${BASE}/pets`]);
  });
});

describe("path serialization (OpenAPI path styles)", () => {
  it("serializes simple values, arrays and objects", () => {
    expect(defaultPathSerializer("/a/{id}", { id: "x y" })).toBe("/a/x%20y");
    expect(defaultPathSerializer("/a/{id}", { id: 5 })).toBe("/a/5");
    expect(defaultPathSerializer("/a/{ids}", { ids: [1, 2, 3] })).toBe("/a/1,2,3");
    expect(defaultPathSerializer("/a/{o}", { o: { x: 1, y: "b c" } })).toBe("/a/x,1,y,b%20c");
    expect(defaultPathSerializer("/a/{o*}", { o: { x: 1, y: 2 } })).toBe("/a/x=1,y=2");
  });

  it("serializes label style ({.name})", () => {
    expect(defaultPathSerializer("/a{.id}", { id: "x y" })).toBe("/a.x%20y");
    expect(defaultPathSerializer("/a{.ids}", { ids: [1, 2] })).toBe("/a.1,2");
    expect(defaultPathSerializer("/a{.ids*}", { ids: [1, 2] })).toBe("/a.1.2");
    expect(defaultPathSerializer("/a{.o}", { o: { x: 1, y: 2 } })).toBe("/a.x,1,y,2");
    expect(defaultPathSerializer("/a{.o*}", { o: { x: 1, y: 2 } })).toBe("/a.x=1.y=2");
  });

  it("serializes matrix style ({;name})", () => {
    expect(defaultPathSerializer("/a{;id}", { id: "x y" })).toBe("/a;id=x%20y");
    expect(defaultPathSerializer("/a{;ids}", { ids: [1, 2] })).toBe("/a;ids=1,2");
    expect(defaultPathSerializer("/a{;ids*}", { ids: [1, 2] })).toBe("/a;ids=1;ids=2");
    expect(defaultPathSerializer("/a{;o}", { o: { x: 1, y: 2 } })).toBe("/a;o=x,1,y,2");
    expect(defaultPathSerializer("/a{;o*}", { o: { x: 1, y: 2 } })).toBe("/a;x=1;y=2");
  });

  it("skips undefined and null values and leaves their placeholders", () => {
    expect(defaultPathSerializer("/a/{id}/{n}", { id: undefined, n: null })).toBe("/a/{id}/{n}");
    expect(defaultPathSerializer("/a/{id}", undefined as never)).toBe("/a/{id}");
  });

  it("takes a path serializer for the client or one request", async () => {
    const { client, requests } = recording(undefined, { pathSerializer: (path) => path.replace("{petId}", "global") });
    await client.GET("/pets/{petId}", { params: { path: { petId: "x" } } });
    await client.GET("/pets/{petId}", {
      params: { path: { petId: "x" } },
      pathSerializer: (path) => path.replace("{petId}", "request"),
    });
    expect(requests.map((r) => r.url)).toEqual([`${BASE}/pets/global`, `${BASE}/pets/request`]);
  });
});

describe("query serialization", () => {
  const serialize = createQuerySerializer();

  it("serializes primitives with percent-encoding, skipping undefined, null and empty arrays", () => {
    expect(serialize({ a: 1, b: "x y", c: true, d: false, e: undefined, f: null, g: [], h: 0, i: "" })).toBe(
      "a=1&b=x%20y&c=true&d=false&h=0&i=",
    );
    expect(serialize({ "a&b": "c=d" })).toBe("a&b=c%3Dd");
    expect(serialize(undefined)).toBe("");
    expect(serialize({})).toBe("");
  });

  it("explodes arrays by default and objects as deepObject", () => {
    expect(serialize({ tags: ["a", "b c"] })).toBe("tags=a&tags=b%20c");
    expect(serialize({ filter: { name: "Rex", tag: "x y" } })).toBe("filter[name]=Rex&filter[tag]=x%20y");
  });

  it("skips undefined and null members of an object and of an array", () => {
    // Compared as pairs: empty members add nothing, whatever separators are left over.
    const pairs = (query: string) => [...new URLSearchParams(query)];
    expect(pairs(serialize({ filter: { name: "Rex", tag: undefined, other: null } }))).toEqual([["filter[name]", "Rex"]]);
    expect(pairs(serialize({ tags: ["a", null, undefined, "b"] }))).toEqual([
      ["tags", "a"],
      ["tags", "b"],
    ]);
  });

  it("supports array styles", () => {
    expect(createQuerySerializer({ array: { style: "form", explode: false } })({ t: ["a", "b c"] })).toBe("t=a,b%20c");
    expect(createQuerySerializer({ array: { style: "spaceDelimited", explode: false } })({ t: ["a", "b"] })).toBe("t=a%20b");
    expect(createQuerySerializer({ array: { style: "pipeDelimited", explode: false } })({ t: ["a", "b"] })).toBe("t=a|b");
    expect(createQuerySerializer({ array: { style: "form", explode: true } })({ t: ["a", "b"] })).toBe("t=a&t=b");
  });

  it("supports object styles", () => {
    expect(createQuerySerializer({ object: { style: "form", explode: true } })({ o: { x: 1, y: "z w" } })).toBe("x=1&y=z%20w");
    expect(createQuerySerializer({ object: { style: "form", explode: false } })({ o: { x: 1, y: "z w" } })).toBe(
      "o=x,1,y,z%20w",
    );
    expect(createQuerySerializer({ object: { style: "deepObject", explode: true } })({ o: { x: 1 } })).toBe("o[x]=1");
  });

  it("keeps reserved characters when allowReserved is true", () => {
    expect(createQuerySerializer({ allowReserved: true })({ a: "b/c:d", t: ["x/y", "z"], o: { k: "v/w" } })).toBe(
      "a=b/c:d&t=x/y&t=z&o[k]=v/w",
    );
    expect(createQuerySerializer({ allowReserved: false })({ a: "b/c:d" })).toBe("a=b%2Fc%3Ad");
  });

  it("refuses values nested more than one level", () => {
    expect(() => serialize({ o: { a: { b: 1 } } })).toThrow("Deeply-nested arrays/objects");
    expect(() => serialize({ t: [["a"]] })).toThrow("Deeply-nested arrays/objects");
  });

  it("serializes the primitive, array and object helpers directly", () => {
    expect(serializePrimitiveParam("a", "b c")).toBe("a=b%20c");
    expect(serializePrimitiveParam("a", undefined as never)).toBe("");
    expect(serializePrimitiveParam("a", null as never)).toBe("");
    expect(serializeArrayParam("a", [1, 2], { style: "form", explode: true })).toBe("a=1&a=2");
    expect(serializeArrayParam("a", "x" as never, { style: "form", explode: true })).toBe("");
    expect(serializeObjectParam("a", { x: 1 }, { style: "deepObject", explode: true })).toBe("a[x]=1");
    expect(serializeObjectParam("a", null as never, { style: "deepObject", explode: true })).toBe("");
  });

  it("builds the final URL from the base, path, path parameters and query", () => {
    const url = createFinalURL("/pets/{petId}", {
      baseUrl: BASE,
      params: { path: { petId: "7" }, query: { limit: 5 } },
      querySerializer: serialize,
      pathSerializer: defaultPathSerializer,
    });
    expect(url).toBe(`${BASE}/pets/7?limit=5`);
    const stripped = createFinalURL("/pets", {
      baseUrl: BASE,
      params: { query: { limit: 5 } },
      querySerializer: () => "?custom=1",
      pathSerializer: defaultPathSerializer,
    });
    expect(stripped).toBe(`${BASE}/pets?custom=1`);
  });

  it("applies a client-wide serializer function or options, and a request's own", async () => {
    const options = recording(undefined, { querySerializer: { array: { style: "form", explode: false }, allowReserved: true } });
    await options.client.GET("/pets", { params: { query: { tags: ["a/b", "c"] } } });
    expect(options.requests[0]!.url).toBe(`${BASE}/pets?tags=a/b,c`);

    // A request's options merge over the client's options.
    await options.client.GET("/pets", {
      params: { query: { tags: ["a", "b"], filter: { name: "x" } } },
      querySerializer: { object: { style: "form", explode: true } },
    });
    expect(options.requests[1]!.url).toBe(`${BASE}/pets?tags=a,b&name=x`);

    const fn = recording(undefined, { querySerializer: (query) => `all=${JSON.stringify(query)}` });
    await fn.client.GET("/pets", { params: { query: { limit: 1 } } });
    expect(decodeURIComponent(fn.requests[0]!.url)).toBe(`${BASE}/pets?all={"limit":1}`);

    // A request's function wins over the client's.
    await fn.client.GET("/pets", { params: { query: { limit: 1 } }, querySerializer: () => "mine=1" });
    expect(fn.requests[1]!.url).toBe(`${BASE}/pets?mine=1`);
  });

  it("does not merge a request's options into a client-wide function", async () => {
    const { client, requests } = recording(undefined, { querySerializer: () => "g=1" });
    await client.GET("/pets", { params: { query: { limit: 3 } }, querySerializer: { allowReserved: true } });
    expect(requests[0]!.url).toBe(`${BASE}/pets?limit=3`);
  });
});

describe("request bodies", () => {
  it("sends an object as JSON with Content-Type: application/json", async () => {
    const { client, requests } = recording(() => json({ id: "1", name: "Rex" }, { status: 201 }));
    await client.POST("/pets", { body: { name: "Rex", tag: "dog" } });
    expect(requests[0]!.method).toBe("POST");
    expect(requests[0]!.headers.get("content-type")).toBe("application/json");
    expect(await requests[0]!.text()).toBe('{"name":"Rex","tag":"dog"}');
  });

  it("sends no body and no Content-Type without a body", async () => {
    const { client, requests } = recording();
    await client.GET("/pets");
    expect(requests[0]!.body).toBeNull();
    expect(requests[0]!.headers.get("content-type")).toBeNull();
  });

  it("serializes a body that is not undefined, null included", async () => {
    const { client, requests } = recording();
    await client.request("post", "/pets", { body: null as never });
    expect(requests[0]!.headers.get("content-type")).toBe("application/json");
    expect(await requests[0]!.text()).toBe("null");
  });

  it("sends application/x-www-form-urlencoded when the Content-Type header says so", async () => {
    const { client, requests } = recording();
    await client.POST("/pets", {
      body: { name: "Rex & co", tag: "x" },
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
    });
    expect(requests[0]!.headers.get("content-type")).toBe("application/x-www-form-urlencoded");
    expect(await requests[0]!.text()).toBe("name=Rex+%26+co&tag=x");
  });

  it("passes FormData through, so the runtime sets the multipart boundary", async () => {
    const { client, requests } = recording();
    const form = new FormData();
    form.set("file", new Blob(["hello"], { type: "text/plain" }), "a.txt");
    await client.PUT("/files/{name}", { params: { path: { name: "a.txt" } }, body: form as never });
    expect(requests[0]!.headers.get("content-type")).toMatch(/^multipart\/form-data; boundary=/);
    expect(await requests[0]!.text()).toContain("hello");
  });

  it("takes a body serializer for the client or one request, given the merged headers", async () => {
    const seen: Array<string | null> = [];
    const { client, requests } = recording(undefined, {
      headers: { "X-Base": "1" },
      bodySerializer: ((body: unknown, headers: Headers) => {
        seen.push(headers.get("x-base"));
        return `global:${JSON.stringify(body)}`;
      }) as never,
    });
    await client.POST("/pets", { body: { name: "a" } });
    await client.POST("/pets", { body: { name: "b" }, bodySerializer: (body) => `request:${(body as { name: string }).name}` });
    expect(await requests[0]!.text()).toBe('global:{"name":"a"}');
    expect(await requests[1]!.text()).toBe("request:b");
    expect(seen).toEqual(["1"]);
    // A serialized body still gets the JSON content type by default.
    expect(requests[0]!.headers.get("content-type")).toBe("application/json");
  });
});

describe("headers", () => {
  it("merges the client's, the request's and the parameters' headers, later ones winning", async () => {
    const { client, requests } = recording(undefined, { headers: { "X-A": "client", "X-B": "client", "X-C": "client" } });
    await client.GET("/pets/{petId}", {
      params: { path: { petId: "1" }, header: { "X-Trace": "param" } },
      headers: { "x-b": "request", "X-Trace": "request" },
    });
    const headers = requests[0]!.headers;
    expect(headers.get("x-a")).toBe("client");
    expect(headers.get("x-b")).toBe("request");
    expect(headers.get("x-c")).toBe("client");
    expect(headers.get("x-trace")).toBe("param");
  });

  it("removes a header set to null, ignores undefined, and appends array values", async () => {
    const { client, requests } = recording(undefined, { headers: { "X-Gone": "1", "X-Kept": "1" } });
    await client.GET("/pets", { headers: { "X-Gone": null, "X-Kept": undefined, "X-Multi": ["a", "b"], "X-Num": 5, "X-Bool": true } });
    const headers = requests[0]!.headers;
    expect(headers.has("x-gone")).toBe(false);
    expect(headers.get("x-kept")).toBe("1");
    expect(headers.get("x-multi")).toBe("a, b");
    expect(headers.get("x-num")).toBe("5");
    expect(headers.get("x-bool")).toBe("true");
  });

  it("accepts a Headers object and lets a request replace the content type", async () => {
    const { client, requests } = recording();
    await client.POST("/pets", { body: { name: "a" }, headers: new Headers({ "content-type": "application/vnd.api+json" }) });
    expect(requests[0]!.headers.get("content-type")).toBe("application/vnd.api+json");
  });

  it("merges headers with later ones winning, case-insensitively", () => {
    const merged = mergeHeaders({ A: "1", b: "1" }, undefined, new Headers({ a: "2" }), { B: null, C: ["x", "y"] });
    expect([...merged.entries()]).toEqual([
      ["a", "2"],
      ["c", "x, y"],
    ]);
    expect([...mergeHeaders("nope" as never, null as never).entries()]).toEqual([]);
  });
});

describe("request options", () => {
  it("uses the client's RequestInit options and lets a request override them", async () => {
    const { client, requests } = recording(undefined, { credentials: "include", cache: "no-store", mode: "cors" });
    await client.GET("/pets");
    await client.GET("/pets", { credentials: "omit", cache: "reload" });
    expect([requests[0]!.credentials, requests[0]!.cache, requests[0]!.mode]).toEqual(["include", "no-store", "cors"]);
    expect([requests[1]!.credentials, requests[1]!.cache, requests[1]!.mode]).toEqual(["omit", "reload", "cors"]);
  });

  it("follows redirects unless told otherwise", async () => {
    const { client, requests } = recording();
    await client.GET("/pets");
    await client.GET("/pets", { redirect: "manual" });
    expect(requests.map((r) => r.redirect)).toEqual(["follow", "manual"]);
  });

  it("names the method from the helper, or uppercases the one given to request()", async () => {
    const { client, requests } = recording();
    await client.GET("/pets");
    await client.request("get", "/pets");
    await client.PATCH("/pets/{petId}", { params: { path: { petId: "1" } } });
    await client.DELETE("/pets/{petId}", { params: { path: { petId: "1" } } });
    await client.HEAD("/files/{name}", { params: { path: { name: "a" } } });
    expect(requests.map((r) => r.method)).toEqual(["GET", "GET", "PATCH", "DELETE", "HEAD"]);
  });

  it("calls a request's own fetch instead of the client's", async () => {
    const clientFetch = vi.fn(async () => json([]));
    const requestFetch = vi.fn(async () => json([{ id: "1", name: "Rex" }]));
    const client = createClient<paths>({ baseUrl: BASE, fetch: clientFetch });
    const { data } = await client.GET("/pets", { fetch: requestFetch });
    expect(clientFetch).not.toHaveBeenCalled();
    expect(requestFetch).toHaveBeenCalledTimes(1);
    expect(data).toEqual([{ id: "1", name: "Rex" }]);
  });

  it("uses globalThis.fetch as it is when the client is created", async () => {
    const original = globalThis.fetch;
    const spy = vi.fn(async () => json([]));
    globalThis.fetch = spy as unknown as typeof fetch;
    try {
      const client = createClient<paths>({ baseUrl: BASE });
      await client.GET("/pets");
      expect(spy).toHaveBeenCalledTimes(1);
    } finally {
      globalThis.fetch = original;
    }
  });

  it("copies unknown init keys onto the request for middleware to read", async () => {
    let seen: unknown;
    const { client } = recording();
    client.use({
      onRequest: ({ request }) => {
        seen = (request as unknown as { retries?: number }).retries;
      },
    });
    await client.GET("/pets", { retries: 3 });
    expect(seen).toBe(3);
  });

  it("passes the abort signal to the request, and the abort to the caller", async () => {
    const controller = new AbortController();
    const { client, requests } = recording(
      (request) =>
        new Promise<Response>((_resolve, reject) => {
          request.signal.addEventListener("abort", () => reject(request.signal.reason));
        }),
    );
    const pending = client.GET("/pets", { signal: controller.signal });
    controller.abort(new DOMException("stop", "AbortError"));
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(requests[0]!.signal.aborted).toBe(true);
  });

  it("rejects at once with a signal that is already aborted", async () => {
    const { client } = recording((request) => {
      request.signal.throwIfAborted();
      return json([]);
    });
    await expect(client.GET("/pets", { signal: AbortSignal.abort() })).rejects.toMatchObject({ name: "AbortError" });
  });

  it("lets a failing fetch reach the caller", async () => {
    const { client } = recording(() => {
      throw new TypeError("fetch failed");
    });
    await expect(client.GET("/pets")).rejects.toThrow("fetch failed");
  });
});

describe("serialization of empty members", () => {
  it("leaves no empty segments where a member is skipped", () => {
    const serialize = createQuerySerializer();
    expect(serialize({ filter: { name: "Rex", tag: undefined, other: null } })).toBe("filter[name]=Rex");
    expect(serialize({ tags: ["a", null, undefined, "b"] })).toBe("tags=a&tags=b");
    expect(serialize({ tags: [null], q: "x" })).toBe("q=x");
    expect(serialize({ o: { a: undefined } })).toBe("");
    expect(createQuerySerializer({ array: { style: "form", explode: false } })({ t: ["a", null, "b"] })).toBe("t=a,b");
  });
});

describe("the core stands alone", () => {
  it("imports nothing and the package has no runtime dependencies", () => {
    const source = readFileSync(new URL("./fetch.ts", import.meta.url), "utf8");
    expect(source).not.toMatch(/^s*(import|export)[^;]*from/m);
    const manifest = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")) as Record<string, unknown>;
    expect(manifest.dependencies).toBeUndefined();
  });
});
