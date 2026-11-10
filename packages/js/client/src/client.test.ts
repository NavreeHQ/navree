import { describe, expect, expectTypeOf, it, vi } from "vitest";
import type { components, paths } from "../test/fixtures/schema.js";
import { createNavreeClient, unwrap, type NavreeClientOptions, type WithQuery } from "./client.js";
import { NavreeError } from "./errors.js";
import { listQuery, type ListQuery } from "./list-query.js";

const BASE = "http://app.test";

const json = (body: unknown, status = 200, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", "x-request-id": "req-1", ...headers },
  });

const deal: components["schemas"]["Deal"] = {
  __type: "Deal",
  id: "d1",
  title: "Agra renewal",
  stage: "won",
  amount_cents: 120000,
  owner_id: null,
};

function recording(respond: (request: Request) => Response | Promise<Response>, options: NavreeClientOptions = {}) {
  const requests: Request[] = [];
  const client = createNavreeClient<paths>({
    baseUrl: BASE,
    ...options,
    fetch: async (request) => {
      requests.push(request.clone());
      return respond(request);
    },
  });
  return { client, requests };
}

describe("cookie mode (the default: a browser behind a backend-for-frontend)", () => {
  it("sends same-origin credentials, Accept: application/json, a request id, and no Authorization", async () => {
    const { client, requests } = recording(() => json({ __type: "User", id: "u1", name: "Anu", email: "a@x.test" }));

    const user = await unwrap(client.GET("/api/auth/me"));

    expect(user.name).toBe("Anu");
    const [request] = requests;
    expect(request!.url).toBe(`${BASE}/api/auth/me`);
    expect(request!.credentials).toBe("same-origin");
    expect(request!.headers.get("accept")).toBe("application/json");
    expect(request!.headers.get("authorization")).toBeNull();
    expect(request!.headers.get("x-request-id")).toMatch(/^[0-9a-f-]{36}$/);
  });

  it("gives every request its own id, or none when requestId is false", async () => {
    const { client, requests } = recording(() => json(deal));
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d2" } } });
    expect(requests[0]!.headers.get("x-request-id")).not.toBe(requests[1]!.headers.get("x-request-id"));

    const quiet = recording(() => json(deal), { requestId: false });
    await quiet.client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    expect(quiet.requests[0]!.headers.get("x-request-id")).toBeNull();

    const custom = recording(() => json(deal), { requestId: () => "trace-7" });
    await custom.client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    expect(custom.requests[0]!.headers.get("x-request-id")).toBe("trace-7");
  });

  it("trims a trailing slash from the base URL and defaults to the page's own origin", async () => {
    const { client, requests } = recording(() => json(deal), { baseUrl: `${BASE}/` });
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    expect(requests[0]!.url).toBe(`${BASE}/api/v1/deals/d1`);
  });
});

describe("bearer mode (servers, scripts, mobile)", () => {
  it("adds the token as Authorization: Bearer and omits cookies", async () => {
    const { client, requests } = recording(() => json(deal), { token: "tok-1" });
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    expect(requests[0]!.headers.get("authorization")).toBe("Bearer tok-1");
    expect(requests[0]!.credentials).toBe("omit");
  });

  it("asks a token function before every request, and sends nothing when it has no token", async () => {
    let current: string | null = "tok-1";
    const token = vi.fn(async () => current);
    const { client, requests } = recording(() => json(deal), { token });

    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    current = null;
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });

    expect(token).toHaveBeenCalledTimes(2);
    expect(requests[0]!.headers.get("authorization")).toBe("Bearer tok-1");
    expect(requests[1]!.headers.get("authorization")).toBeNull();
  });
});

describe("workspaces", () => {
  it("sends X-Workspace from a value or a function", async () => {
    let slug: string | undefined = "agra";
    const { client, requests } = recording(() => json(deal), { workspace: () => slug });
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    slug = undefined;
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });

    expect(requests[0]!.headers.get("x-workspace")).toBe("agra");
    expect(requests[1]!.headers.get("x-workspace")).toBeNull();
  });

  it("uses the app's own header name (workspace.header)", async () => {
    const { client, requests } = recording(() => json(deal), { workspace: "agra", workspaceHeader: "X-Team" });
    await client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } });
    expect(requests[0]!.headers.get("x-team")).toBe("agra");
  });

  it("lets a request override the workspace and other headers", async () => {
    const { client, requests } = recording(() => json(deal), { workspace: "agra", headers: { "Accept-Language": "fr" } });
    await client.GET("/api/v1/deals/{id}", {
      params: { path: { id: "d1" } },
      headers: { "X-Workspace": "gulati" },
    });
    expect(requests[0]!.headers.get("x-workspace")).toBe("gulati");
    expect(requests[0]!.headers.get("accept-language")).toBe("fr");
  });
});

describe("list queries", () => {
  type Paths = WithQuery<paths, { "/api/v1/deals": ListQuery }>;

  it("serializes filters, sort and paging in list-query format", async () => {
    const requests: Request[] = [];
    const client = createNavreeClient<Paths>({
      baseUrl: BASE,
      fetch: async (request) => {
        requests.push(request);
        return json({ data: [deal], meta: { current_page: 2, per_page: 25, total: 26, last_page: 2 } });
      },
    });

    const query = listQuery().where("stage", "won").where("amount_cents", "gte", 1000).sort("-created_at").page(2).perPage(25);
    const page = await unwrap(client.GET("/api/v1/deals", { params: { query: query.toQuery() } }));

    expect(page.data[0]!.title).toBe("Agra renewal");
    expect(decodeURIComponent(new URL(requests[0]!.url).search)).toBe(
      "?filter[stage]=won&filter[amount_cents][gte]=1000&sort=-created_at&page=2&per_page=25",
    );
  });
});

describe("errors", () => {
  it("unwrap throws a NavreeError with the code, field errors and request id", async () => {
    const { client } = recording(() =>
      json(
        {
          statusCode: 400,
          message: ["title should not be empty"],
          error: "Bad Request",
          code: "validation_failed",
          errors: { title: ["title should not be empty"] },
        },
        400,
        { "x-request-id": "req-400" },
      ),
    );

    const error = await unwrap(client.POST("/api/v1/deals", { body: { title: "" } })).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(NavreeError);
    const e = error as NavreeError;
    expect(e.status).toBe(400);
    expect(e.code).toBe("validation_failed");
    expect(e.fieldError("title")).toBe("title should not be empty");
    expect(e.requestId).toBe("req-400");
  });

  it("unwrap turns a failed fetch into network_error", async () => {
    const { client } = recording(() => {
      throw new TypeError("fetch failed");
    });
    const error = await unwrap(client.GET("/api/auth/me")).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(NavreeError);
    expect((error as NavreeError).code).toBe("network_error");
  });

  it("unwrap returns undefined for a 204", async () => {
    const { client } = recording(() => new Response(null, { status: 204 }));
    await expect(unwrap(client.DELETE("/api/v1/deals/{id}", { params: { path: { id: "d1" } } }))).resolves.toBeUndefined();
  });

  it("calls onUnauthorized on a 401 and onForbidden with the parsed error on a 403", async () => {
    const onUnauthorized = vi.fn();
    const onForbidden = vi.fn();
    let status = 401;
    const { client } = recording(
      () =>
        json(
          { statusCode: status, message: status === 401 ? "Unauthorized" : "Demo workspaces are read-only.", error: "x", code: status === 401 ? "unauthenticated" : "demo_read_only" },
          status,
        ),
      { onUnauthorized, onForbidden },
    );

    await client.GET("/api/auth/me");
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
    expect(onUnauthorized.mock.calls[0]![0]).toBeInstanceOf(Response);

    status = 403;
    const result = await client.GET("/api/auth/me");
    expect(onForbidden).toHaveBeenCalledTimes(1);
    expect((onForbidden.mock.calls[0]![0] as NavreeError).code).toBe("demo_read_only");
    // The body is still there for the caller.
    expect((result.error as { code: string }).code).toBe("demo_read_only");
  });
});

describe("raw requests (exports, streams, channel authorization)", () => {
  it("send the same headers and run the same hooks", async () => {
    const onUnauthorized = vi.fn();
    const { client, requests } = recording(
      (request) =>
        request.url.endsWith("/export")
          ? new Response("id,title\r\n", { headers: { "content-type": "text/csv" } })
          : new Response(null, { status: 401 }),
      { workspace: "agra", onUnauthorized },
    );

    const csv = await client.raw("/api/v1/deals/export", { headers: { Accept: "text/csv" } });
    expect(await csv.text()).toBe("id,title\r\n");
    expect(requests[0]!.url).toBe(`${BASE}/api/v1/deals/export`);
    expect(requests[0]!.headers.get("accept")).toBe("text/csv");
    expect(requests[0]!.headers.get("x-workspace")).toBe("agra");
    expect(requests[0]!.credentials).toBe("same-origin");

    await client.raw("/broadcasting/auth", { method: "POST" });
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });
});

describe("types", () => {
  it("types data from the OpenAPI document", async () => {
    const client = createNavreeClient<paths>({ baseUrl: BASE, fetch: async () => json(deal) });
    const loaded = await unwrap(client.GET("/api/v1/deals/{id}", { params: { path: { id: "d1" } } }));
    expectTypeOf(loaded).toEqualTypeOf<components["schemas"]["Deal"]>();
    expect(loaded.id).toBe("d1");
  });
});
