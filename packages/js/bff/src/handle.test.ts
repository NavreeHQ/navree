import { describe, expect, it } from "vitest";
import { ACCESS_COOKIE, CHALLENGE_COOKIE, createBff, REFRESH_COOKIE, WORKSPACE_COOKIE } from "./bff.js";
import { browserRequest, fakeNavree, readSetCookies, tokenPair, type BrowserRequestOptions } from "./testing.js";

const API = "http://navree.test";
const APP = "http://app.test";
const request = (path: string, options: BrowserRequestOptions = {}) => browserRequest(APP, path, options);
const user = { __type: "User", id: "u1", name: "Anu" };

describe("bff.handle: one entry point for every BFF route", () => {
  const navree = () =>
    fakeNavree({
      "POST /api/auth/login": () => Response.json({ user, ...tokenPair(1) }),
      "POST /api/auth/2fa/verify": () => Response.json({ user, ...tokenPair(2) }),
      "POST /api/auth/logout": () => new Response(null, { status: 204 }),
      "POST /api/auth/refresh": () => Response.json(tokenPair(3)),
      "GET /api/v1/deals": (call) => Response.json({ auth: call.headers.get("authorization") }),
      "POST /broadcasting/auth": () => Response.json({ auth: "key:sig" }),
    });

  it("routes sign-in, two-factor, sign-out and refresh to their handlers", async () => {
    const api = navree();
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const login = await bff.handle(request("/api/auth/login", { method: "POST", body: { email: "a", password: "b" } }));
    expect(await login.json()).toEqual({ user });
    expect(readSetCookies(login)[ACCESS_COOKIE]!.value).toBe("AT1");

    const verify = await bff.handle(
      request("/api/auth/2fa/verify", { method: "POST", cookies: { [CHALLENGE_COOKIE]: "C" }, body: { code: "1" } }),
    );
    expect(readSetCookies(verify)[ACCESS_COOKIE]!.value).toBe("AT2");

    const refresh = await bff.handle(request("/api/auth/refresh", { method: "POST", cookies: { [REFRESH_COOKIE]: "RT2" } }));
    expect(refresh.status).toBe(204);
    expect(readSetCookies(refresh)[REFRESH_COOKIE]!.value).toBe("RT3");

    const logout = await bff.handle(
      request("/api/auth/logout", { method: "POST", cookies: { [ACCESS_COOKIE]: "AT3", [REFRESH_COOKIE]: "RT3" } }),
    );
    expect(logout.status).toBe(204);

    expect(api.calls.map((c) => `${c.method} ${new URL(c.url).pathname}`)).toEqual([
      "POST /api/auth/login",
      "POST /api/auth/2fa/verify",
      "POST /api/auth/refresh",
      "POST /api/auth/logout",
    ]);
  });

  it("proxies everything else at its own path, channel authorization included", async () => {
    const api = navree();
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const deals = await bff.handle(request("/api/v1/deals?page=2", { cookies: { [ACCESS_COOKIE]: "AT9" } }));
    expect(await deals.json()).toEqual({ auth: "Bearer AT9" });
    expect(api.calls[0]!.url).toBe(`${API}/api/v1/deals?page=2`);

    const channel = await bff.handle(
      request("/broadcasting/auth", { method: "POST", cookies: { [ACCESS_COOKIE]: "AT9" }, body: { socket_id: "1.2" } }),
    );
    expect(await channel.json()).toEqual({ auth: "key:sig" });
  });

  it("rewrites the upstream path when the BFF is mounted elsewhere", async () => {
    const api = navree();
    const bff = createBff({
      apiUrl: API,
      fetch: api.fetch,
      upstreamPath: (pathname) => pathname.replace(/^\/bff/, ""),
    });

    await bff.handle(request("/bff/api/v1/deals", { cookies: { [ACCESS_COOKIE]: "AT1" } }));
    expect(api.calls[0]!.url).toBe(`${API}/api/v1/deals`);
  });

  it("serves the auth routes under another prefix, with the challenge cookie scoped to it", async () => {
    const api = fakeNavree({
      "POST /v2/auth/login": () => Response.json({ two_factor_required: true, challenge_token: "C", expires_in: 120 }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, authPrefix: "/v2/auth" });

    const login = await bff.handle(browserRequest(APP, "/v2/auth/login", { method: "POST", body: {} }));
    expect(await login.json()).toEqual({ two_factor_required: true, expires_in: 120 });
    expect(readSetCookies(login)[CHALLENGE_COOKIE]!.attrs).toContain("Path=/v2/auth/2fa");
  });

  it("uses the app's own cookie names", async () => {
    const api = navree();
    const bff = createBff({ apiUrl: API, fetch: api.fetch, cookieNames: { access: "agra_at", refresh: "agra_rt" } });

    const login = await bff.handle(request("/api/auth/login", { method: "POST", body: {} }));
    expect(Object.keys(readSetCookies(login))).toEqual(["agra_at", "agra_rt"]);

    await bff.handle(request("/api/v1/deals", { cookies: { agra_at: "AT5" } }));
    expect(api.calls[1]!.headers.get("authorization")).toBe("Bearer AT5");
  });
});

describe("workspaces through the BFF", () => {
  const echo = () =>
    fakeNavree({ "GET /api/v1/deals": (call) => Response.json({ workspace: call.headers.get("x-workspace") }) });

  it("forwards the browser's X-Workspace header", async () => {
    const api = echo();
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const response = await bff.proxy(request("/api/v1/deals", { headers: { "x-workspace": "agra" } }), "/api/v1/deals");
    expect(await response.json()).toEqual({ workspace: "agra" });
  });

  it("falls back to the workspace cookie, so server-rendered pages and plain links are workspace-aware", async () => {
    const api = echo();
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const fromCookie = await bff.proxy(request("/api/v1/deals", { cookies: { [WORKSPACE_COOKIE]: "gulati" } }), "/api/v1/deals");
    const headerWins = await bff.proxy(
      request("/api/v1/deals", { cookies: { [WORKSPACE_COOKIE]: "gulati" }, headers: { "x-workspace": "agra" } }),
      "/api/v1/deals",
    );
    expect(await fromCookie.json()).toEqual({ workspace: "gulati" });
    expect(await headerWins.json()).toEqual({ workspace: "agra" });
  });

  it("uses the app's workspace header name", async () => {
    const api = fakeNavree({ "GET /api/v1/deals": (call) => Response.json({ team: call.headers.get("x-team") }) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, workspaceHeader: "X-Team" });
    const response = await bff.proxy(request("/api/v1/deals", { headers: { "x-team": "agra" } }), "/api/v1/deals");
    expect(await response.json()).toEqual({ team: "agra" });
  });
});

describe("streams and pages through the BFF", () => {
  it("streams server-sent events as they arrive and passes Last-Event-ID on", async () => {
    const encoder = new TextEncoder();
    let push: (text: string) => void = () => undefined;
    let close: () => void = () => undefined;
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        push = (text) => controller.enqueue(encoder.encode(text));
        close = () => controller.close();
      },
    });
    const api = fakeNavree({
      "GET /api/operations/op1/events": () =>
        new Response(body, { headers: { "content-type": "text/event-stream", "cache-control": "no-cache" } }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const response = await bff.proxy(
      request("/api/operations/op1/events", {
        cookies: { [ACCESS_COOKIE]: "AT1" },
        headers: { accept: "text/event-stream", "last-event-id": "4" },
      }),
      "/api/operations/op1/events",
    );

    expect(response.headers.get("content-type")).toBe("text/event-stream");
    expect(response.headers.get("cache-control")).toBe("no-cache");
    expect(api.calls[0]!.headers.get("last-event-id")).toBe("4");
    expect(api.calls[0]!.headers.get("accept")).toBe("text/event-stream");

    const reader = response.body!.getReader();
    push("event: operation\ndata: {\"status\":\"running\"}\n\n");
    const first = await reader.read();
    expect(new TextDecoder().decode(first.value)).toContain("running");
    push("event: operation\ndata: {\"status\":\"succeeded\"}\n\n");
    close();
    const second = await reader.read();
    expect(new TextDecoder().decode(second.value)).toContain("succeeded");
  });

  it("passes the security headers of server-rendered pages (a tool mounted at /admin) through", async () => {
    const api = fakeNavree({
      "GET /admin/users": () =>
        new Response("<!doctype html><title>Users</title>", {
          headers: {
            "content-type": "text/html; charset=utf-8",
            "content-security-policy": "default-src 'self'",
            "x-frame-options": "DENY",
            "x-content-type-options": "nosniff",
            "referrer-policy": "same-origin",
            "set-cookie": "upstream=1",
          },
        }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const response = await bff.handle(request("/admin/users", { cookies: { [ACCESS_COOKIE]: "AT1" } }));

    expect(await response.text()).toContain("<title>Users</title>");
    expect(response.headers.get("content-security-policy")).toBe("default-src 'self'");
    expect(response.headers.get("x-frame-options")).toBe("DENY");
    expect(response.headers.get("x-content-type-options")).toBe("nosniff");
    expect(response.headers.get("referrer-policy")).toBe("same-origin");
    // The API's own cookies never reach the browser through the BFF.
    expect(response.headers.getSetCookie()).toEqual([]);
    expect(api.calls[0]!.headers.get("authorization")).toBe("Bearer AT1");
  });
});

describe("error codes of the BFF's own refusals", () => {
  it("a failed refresh is session_expired, an expired two-factor attempt two_factor_expired", async () => {
    const api = fakeNavree({
      "POST /api/auth/refresh": () =>
        Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized", code: "unauthenticated" }, { status: 401 }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const refresh = await bff.refreshSession(request("/api/auth/refresh", { method: "POST", cookies: { [REFRESH_COOKIE]: "RT1" } }));
    expect(await refresh.json()).toMatchObject({ statusCode: 401, code: "session_expired" });

    const verify = await bff.verifyTwoFactor(request("/api/auth/2fa/verify", { method: "POST", body: { code: "1" } }));
    expect(await verify.json()).toMatchObject({ statusCode: 401, code: "two_factor_expired" });
  });

  it("an answer the BFF can't use is unexpected_response", async () => {
    const api = fakeNavree({ "POST /api/auth/login": () => Response.json({ hello: "world" }) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const login = await bff.login(request("/api/auth/login", { method: "POST", body: {} }));
    expect(login.status).toBe(502);
    expect(await login.json()).toMatchObject({ code: "unexpected_response" });
  });
});
