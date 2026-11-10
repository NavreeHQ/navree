import { describe, expectTypeOf, it } from "vitest";
import type { Account, NewPet, Pet, Problem, paths } from "../test/fixtures/typed.js";
import { createNavreeClient, type NavreeClient, type WithQuery } from "./client.js";
import { createClient, type Client, type Middleware } from "./fetch.js";
import type { ListQuery } from "./list-query.js";

const client = createClient<paths>({ baseUrl: "http://api.test" });

describe("path parameters", () => {
  it("requires params.path when the operation has path parameters", () => {
    // @ts-expect-error params.path is required
    void client.GET("/pets/{petId}", {});
    // @ts-expect-error petId is a string
    void client.GET("/pets/{petId}", { params: { path: { petId: 1 } } });
    void client.GET("/pets/{petId}", { params: { path: { petId: "1" } } });
  });

  it("leaves the init optional when nothing is required", () => {
    void client.GET("/pets");
    void client.GET("/pets", {});
    void client.GET("/pets", { params: {} });
  });

  it("takes only paths that have the method", () => {
    // @ts-expect-error /accounts has no GET
    void client.GET("/accounts");
    // @ts-expect-error /search has no POST
    void client.POST("/search", { params: { query: { q: "x" } } });
    // @ts-expect-error not a path
    void client.GET("/nope");
    // @ts-expect-error /pets has no PATCH
    void client.PATCH("/pets");
    void client.PATCH("/pets/{petId}", { params: { path: { petId: "1" } } });
  });

});

describe("query and header parameters", () => {
  it("types the query, with optional members", () => {
    void client.GET("/pets", { params: { query: { limit: 5, tags: ["a"], filter: { name: "x" }, q: null } } });
    // @ts-expect-error limit is a number
    void client.GET("/pets", { params: { query: { limit: "5" } } });
    // @ts-expect-error tags is a list
    void client.GET("/pets", { params: { query: { tags: "a" } } });
    // @ts-expect-error unknown query parameter
    void client.GET("/pets", { params: { query: { nope: 1 } } });
  });

  it("requires params.query when a query parameter is required", () => {
    // @ts-expect-error q is required
    void client.GET("/search");
    // @ts-expect-error q is required
    void client.GET("/search", { params: { query: { page: 1 } } });
    void client.GET("/search", { params: { query: { q: "x" } } });
  });

  it("types header parameters", () => {
    void client.GET("/pets/{petId}", { params: { path: { petId: "1" }, header: { "X-Trace": "t" } } });
    // @ts-expect-error X-Trace is a string
    void client.GET("/pets/{petId}", { params: { path: { petId: "1" }, header: { "X-Trace": 1 } } });
  });
});

describe("request bodies", () => {
  it("requires a body that is required, typed from the document", () => {
    // @ts-expect-error body is required
    void client.POST("/pets");
    // @ts-expect-error body is required
    void client.POST("/pets", {});
    // @ts-expect-error name is required
    void client.POST("/pets", { body: { tag: "dog" } });
    // @ts-expect-error name is a string
    void client.POST("/pets", { body: { name: 1 } });
    void client.POST("/pets", { body: { name: "Rex" } });
    expectTypeOf<Parameters<typeof client.POST<"/pets", { body: NewPet }>>[1]>().toMatchTypeOf<{ body: NewPet }>();
  });

  it("makes an optional body optional", () => {
    void client.PATCH("/pets/{petId}", { params: { path: { petId: "1" } } });
    void client.PATCH("/pets/{petId}", { params: { path: { petId: "1" } }, body: { tag: "x" } });
    // @ts-expect-error tag is a string
    void client.PATCH("/pets/{petId}", { params: { path: { petId: "1" } }, body: { tag: 1 } });
  });

  it("refuses a body where the operation has none", () => {
    // @ts-expect-error GET has no body
    void client.GET("/pets", { body: { name: "x" } });
  });

  it("removes read-only properties from a body and write-only ones from the data", async () => {
    // `id` is read-only: it can't be sent. `password` is write-only: it is sent but never comes back.
    void client.POST("/accounts", { body: { email: "a@x.test", password: "pw" } });
    // @ts-expect-error id is read-only
    void client.POST("/accounts", { body: { id: "1", email: "a@x.test", password: "pw" } });
    // @ts-expect-error password is required in a request
    void client.POST("/accounts", { body: { email: "a@x.test" } });

    const { data } = await client.POST("/accounts", { body: { email: "a@x.test", password: "pw" } });
    expectTypeOf(data).toEqualTypeOf<{ id: string; email: string } | undefined>();
    expectTypeOf<Account>().toHaveProperty("password");
  });

  it("takes FormData-like bodies through the body type of the media type", () => {
    void client.PUT("/files/{name}", { params: { path: { name: "a" } }, body: { file: "x" } });
    // @ts-expect-error file is required
    void client.PUT("/files/{name}", { params: { path: { name: "a" } }, body: {} });
  });
});

describe("responses", () => {
  it("types data from the success status and error from the error statuses", async () => {
    const result = await client.GET("/pets/{petId}", { params: { path: { petId: "1" } } });
    if (result.data) {
      expectTypeOf(result.data).toEqualTypeOf<Pet>();
      expectTypeOf(result.error).toEqualTypeOf<undefined>();
    } else {
      expectTypeOf(result.data).toEqualTypeOf<undefined>();
      expectTypeOf(result.error).toEqualTypeOf<Problem | { trace: string }>();
    }
    expectTypeOf(result.response).toEqualTypeOf<Response>();
  });

  it("narrows between data and error", async () => {
    const { data, error } = await client.GET("/pets/{petId}", { params: { path: { petId: "1" } } });
    if (error) {
      expectTypeOf(data).toEqualTypeOf<undefined>();
    } else {
      expectTypeOf(data).toEqualTypeOf<Pet>();
    }
  });

  it("uses 201 for a created resource and every error status for the error", async () => {
    const { data, error } = await client.POST("/pets", { body: { name: "Rex" } });
    expectTypeOf(data).toEqualTypeOf<Pet | undefined>();
    expectTypeOf(error).toEqualTypeOf<Problem | { fields: Record<string, string[]> } | undefined>();
  });

  it("falls back to the default response for the error", async () => {
    const { data, error } = await client.GET("/pets");
    expectTypeOf(data).toEqualTypeOf<Pet[] | undefined>();
    expectTypeOf(error).toEqualTypeOf<Problem | undefined>();
  });

  it("types an empty response as undefined", async () => {
    const { data, error } = await client.DELETE("/pets/{petId}", { params: { path: { petId: "1" } } });
    expectTypeOf(data).toEqualTypeOf<undefined>();
    expectTypeOf(error).toEqualTypeOf<Problem | undefined>();
  });

  it("reads status ranges: 2XX as success and 4XX as error", async () => {
    const stored = await client.PUT("/files/{name}", { params: { path: { name: "a" } }, body: { file: "x" } });
    expectTypeOf(stored.data).toEqualTypeOf<{ stored: true } | undefined>();
    expectTypeOf(stored.error).toEqualTypeOf<undefined>();

    const file = await client.GET("/files/{name}", { params: { path: { name: "a" } } });
    expectTypeOf(file.error).toEqualTypeOf<Problem | undefined>();
  });

  it("unions the content types of a response", async () => {
    const { data } = await client.GET("/files/{name}", { params: { path: { name: "a" } } });
    expectTypeOf(data).toEqualTypeOf<string | { name: string; size: number } | undefined>();
  });

  it("retypes data with parseAs", async () => {
    const path = { params: { path: { name: "a" } } };
    const text = await client.GET("/files/{name}", { ...path, parseAs: "text" });
    expectTypeOf(text.data).toEqualTypeOf<string | undefined>();
    const blob = await client.GET("/files/{name}", { ...path, parseAs: "blob" });
    expectTypeOf(blob.data).toEqualTypeOf<Blob | undefined>();
    const buffer = await client.GET("/files/{name}", { ...path, parseAs: "arrayBuffer" });
    expectTypeOf(buffer.data).toEqualTypeOf<ArrayBuffer | undefined>();
    const stream = await client.GET("/files/{name}", { ...path, parseAs: "stream" });
    expectTypeOf(stream.data).toEqualTypeOf<ReadableStream<Uint8Array<ArrayBuffer>> | null | undefined>();
  });

  it("types request() like the method helpers", async () => {
    const { data } = await client.request("get", "/pets/{petId}", { params: { path: { petId: "1" } } });
    expectTypeOf(data).toEqualTypeOf<Pet | undefined>();
    // @ts-expect-error /accounts has no get
    void client.request("get", "/accounts");
  });
});

describe("client options and hooks", () => {
  it("types the serializers, headers and middleware", () => {
    createClient<paths>({
      baseUrl: "x",
      headers: { A: "1", B: ["a", "b"], C: null, D: undefined, E: 1, F: true },
      querySerializer: { array: { style: "form", explode: false }, object: { style: "deepObject", explode: true }, allowReserved: true },
      bodySerializer: (body) => JSON.stringify(body),
      pathSerializer: (path) => path,
      credentials: "include",
    });
    createClient<paths>({ querySerializer: (query) => String(query) });
    // @ts-expect-error not a style
    createClient<paths>({ querySerializer: { array: { style: "nope", explode: true } } });
  });

  it("types middleware hooks", () => {
    const client = createClient<paths>();
    client.use({
      onRequest({ request, schemaPath, params, options, id }) {
        expectTypeOf(request).toEqualTypeOf<Request>();
        expectTypeOf(schemaPath).toEqualTypeOf<string>();
        expectTypeOf(params.query).toEqualTypeOf<Record<string, unknown> | undefined>();
        expectTypeOf(id).toEqualTypeOf<string>();
        expectTypeOf(options.baseUrl).toEqualTypeOf<string>();
        return request;
      },
      onResponse({ response }) {
        expectTypeOf(response).toEqualTypeOf<Response>();
      },
      onError({ error }) {
        expectTypeOf(error).toEqualTypeOf<unknown>();
      },
    });
    // @ts-expect-error a middleware needs a hook
    client.use({});
    expectTypeOf<Middleware>().not.toBeAny();
  });

  it("has every HTTP method", () => {
    expectTypeOf(client).toHaveProperty("GET");
    expectTypeOf(client).toHaveProperty("PUT");
    expectTypeOf(client).toHaveProperty("POST");
    expectTypeOf(client).toHaveProperty("DELETE");
    expectTypeOf(client).toHaveProperty("OPTIONS");
    expectTypeOf(client).toHaveProperty("HEAD");
    expectTypeOf(client).toHaveProperty("PATCH");
    expectTypeOf(client).toHaveProperty("TRACE");
    expectTypeOf(client).toHaveProperty("request");
    expectTypeOf(client).toHaveProperty("use");
    expectTypeOf(client).toHaveProperty("eject");
    expectTypeOf(client).toEqualTypeOf<Client<paths>>();
  });
});

describe("the Navree client", () => {
  const navree = createNavreeClient<paths>();

  it("is the typed client plus raw()", async () => {
    expectTypeOf(navree).toEqualTypeOf<NavreeClient<paths>>();
    expectTypeOf(navree.raw).toEqualTypeOf<(path: string, init?: RequestInit) => Promise<Response>>();
    const { data } = await navree.GET("/pets/{petId}", { params: { path: { petId: "1" } } });
    expectTypeOf(data).toEqualTypeOf<Pet | undefined>();
    // @ts-expect-error petId is required
    void navree.GET("/pets/{petId}");
  });

  it("swaps the query of a route with WithQuery", async () => {
    type Listed = WithQuery<paths, { "/pets": ListQuery }>;
    const listed = createNavreeClient<Listed>();
    void listed.GET("/pets", { params: { query: { filter: { name: "x" } } } });
    const { data } = await listed.GET("/pets");
    expectTypeOf(data).toEqualTypeOf<Pet[] | undefined>();
    // The route that was not swapped keeps its query.
    void listed.GET("/search", { params: { query: { q: "x" } } });
    // @ts-expect-error q is still required
    void listed.GET("/search");
  });
});
