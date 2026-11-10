import { describe, expect, it } from "vitest";
import { BASE, recording } from "../test/fetch-helpers.js";
import {
  createClient,
  createFinalURL,
  createQuerySerializer,
  defaultPathSerializer,
  serializeObjectParam,
} from "./fetch.js";

// Parameters come from people: a value, an object key or a query name must stay inside the part of
// the URL it is written into.

const template = "/users/{id}/profile";

const urlOf = (path: unknown, serialize = defaultPathSerializer, base = "https://api.example.com/v1") =>
  createFinalURL(template, {
    baseUrl: base,
    params: { path: { id: path } as Record<string, unknown> },
    querySerializer: createQuerySerializer(),
    pathSerializer: serialize,
  });

describe("dot segments in path parameters", () => {
  it("refuses a value that is . or .. wherever it ends up as a whole segment", () => {
    for (const id of [".", ".."]) {
      expect(() => urlOf(id)).toThrow(/dot segment/);
      expect(() => urlOf([id])).toThrow(/dot segment/);
    }
    expect(() => defaultPathSerializer("/a/{.id}/x", { id: "." })).toThrow(/dot segment/);
    expect(() => defaultPathSerializer("/a/{id}/x", { id: { ".": "" } })).not.toThrow();
    expect(() => defaultPathSerializer("/a/{id*}/x", { id: [".."] })).toThrow(/dot segment/);
    expect(() => defaultPathSerializer("/{id}", { id: ".." })).toThrow(/dot segment/);
    expect(() => defaultPathSerializer("{id}/x", { id: ".." })).toThrow(/dot segment/);
    expect(() => defaultPathSerializer("/a/{id}", { id: ".." })).toThrow(/dot segment/);
  });

  it("refuses the dot segments a URL reads in their encoded forms", () => {
    // `%2e` is a dot to a URL, so a template that spells one next to a value is as bad.
    expect(() => defaultPathSerializer("/a/%2e{id}/x", { id: "." })).toThrow(/dot segment/);
    expect(() => defaultPathSerializer("/a/%2E{id}/x", { id: "%2e" })).not.toThrow();
  });

  it("writes what is not a whole dot segment as before", () => {
    expect(urlOf("...")).toBe("https://api.example.com/v1/users/.../profile");
    expect(urlOf(".a")).toBe("https://api.example.com/v1/users/.a/profile");
    expect(urlOf("a.")).toBe("https://api.example.com/v1/users/a./profile");
    // A percent sign is encoded, so `%2e%2e` stays text.
    expect(urlOf("%2e%2e")).toBe("https://api.example.com/v1/users/%252e%252e/profile");
    expect(urlOf(["..", "x"])).toBe("https://api.example.com/v1/users/..,x/profile");
    expect(defaultPathSerializer("/a{;id}", { id: ".." })).toBe("/a;id=..");
  });

  it("is refused through a call and sends nothing", async () => {
    const { client, requests } = recording();
    await expect(
      client.GET("/pets/{petId}", { params: { path: { petId: ".." } } }),
    ).rejects.toThrow(/dot segment/);
    expect(requests).toHaveLength(0);
  });
});

describe("object keys in path parameters", () => {
  it("encodes keys as it encodes values", () => {
    expect(defaultPathSerializer("/u/{o}/p", { o: { "../../admin?x": "y" } })).toBe("/u/..%2F..%2Fadmin%3Fx,y/p");
    expect(defaultPathSerializer("/u/{o*}/p", { o: { "a/b#c": "y" } })).toBe("/u/a%2Fb%23c=y/p");
    expect(defaultPathSerializer("/u{.o*}/p", { o: { "a/b": "y" } })).toBe("/u.a%2Fb=y/p");
    expect(defaultPathSerializer("/u{;o*}/p", { o: { "a b;c": "y" } })).toBe("/u;a%20b%3Bc=y/p");
  });

  it("keeps plain keys as they are", () => {
    expect(serializeObjectParam("o", { x: 1, y: "b c" }, { style: "simple", explode: false })).toBe("x,1,y,b%20c");
  });
});

describe("query names and keys", () => {
  const serialize = createQuerySerializer();

  it("encodes a name, so it cannot add parameters", () => {
    expect(serialize({ "a&admin=true#": "1" })).toBe("a%26admin%3Dtrue%23=1");
    expect(serialize({ "a=1&admin": "true" })).toBe("a%3D1%26admin=true");
    expect(serialize({ "n&m=1": ["1", "2"] })).toBe("n%26m%3D1=1&n%26m%3D1=2");
  });

  it("encodes the keys of an object, brackets around them stay", () => {
    expect(serialize({ f: { "x]=1&admin=true&y[z": "v" } })).toBe("f[x%5D%3D1%26admin%3Dtrue%26y%5Bz]=v");
    expect(serialize({ f: { name: "Rex", "a b": 1 } })).toBe("f[name]=Rex&f[a%20b]=1");
    expect(createQuerySerializer({ object: { style: "form", explode: true } })({ f: { "k&m=1": "v" } })).toBe(
      "k%26m%3D1=v",
    );
    expect(createQuerySerializer({ object: { style: "form", explode: false } })({ f: { "k&m=1": "v" } })).toBe(
      "f=k%26m%3D1,v",
    );
  });

  it("encodes the name of a parameter the call sends", async () => {
    const { client, requests } = recording();
    await client.GET("/search", { params: { query: { "q&admin=true": "x" } as never } });
    expect(requests[0]!.url).toBe(`${BASE}/search?q%26admin%3Dtrue=x`);
  });

  it("keeps the plain names and values the way they were written", () => {
    expect(serialize({ a: 1, b: "x y", "c-d_e.f~g": true })).toBe("a=1&b=x%20y&c-d_e.f~g=true");
    expect(serialize({ filter: { name: "Rex", tag: undefined } })).toBe("filter[name]=Rex");
  });
});

describe("a path that starts a URL with //", () => {
  const empty = (path: Record<string, unknown>, base = "", pattern = "/{a}/{b}/x") =>
    createFinalURL(pattern, {
      baseUrl: base,
      params: { path },
      querySerializer: createQuerySerializer(),
      pathSerializer: defaultPathSerializer,
    });

  it("refuses an empty first segment, which names a host", () => {
    expect(() => empty({ a: "", b: "evil.example" })).toThrow(/start with `\/\/`/);
    expect(() => empty({ a: "" }, "", "/{a}/x")).toThrow(/start with `\/\/`/);
    expect(() => empty({ a: "", b: "evil.example" }, "", "/{a}/{b}")).toThrow(/start with `\/\/`/);
  });

  it("allows an empty segment after a host and a template that starts with //", () => {
    expect(empty({ a: "", b: "x" }, "https://api.example.com")).toBe("https://api.example.com//x/x");
    expect(empty({ a: "p" }, "", "//host.test/{a}")).toBe("//host.test/p");
    expect(empty({ a: "p", b: "q" })).toBe("/p/q/x");
  });

  it("is refused through a call and sends nothing", async () => {
    const requests: Request[] = [];
    const api = createClient<Record<string, never>>({
      baseUrl: "",
      fetch: async (request) => {
        requests.push(request);
        return new Response("{}");
      },
    });
    const call = (api as unknown as { GET: (path: string, options: unknown) => Promise<unknown> }).GET;
    await expect(call("/{a}/x", { params: { path: { a: "" } } })).rejects.toThrow(/start with `\/\/`/);
    expect(requests).toHaveLength(0);
  });
});
