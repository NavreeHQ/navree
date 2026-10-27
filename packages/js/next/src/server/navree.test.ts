// @vitest-environment node
import { ACCESS_COOKIE, REFRESH_COOKIE, WORKSPACE_COOKIE } from "@navree/bff";
import { browserRequest, fakeNavree, readSetCookies, tokenPair, unsignedJwt } from "@navree/bff/testing";
import { unwrap } from "@navree/client";
import { NextRequest } from "next/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createNavree } from "./navree.js";

const API = "http://navree.test";
const APP = "http://app.test";
const user = { __type: "User", id: "u1", name: "Ada" };

/** The cookies `next/headers` returns in a server component; set per test. */
const jar = vi.hoisted(() => ({ cookies: new Map<string, string>() }));
vi.mock("next/headers", () => ({
  cookies: async () => ({
    get: (name: string) => (jar.cookies.has(name) ? { name, value: jar.cookies.get(name)! } : undefined),
  }),
}));

beforeEach(() => jar.cookies.clear());

function api() {
  return fakeNavree({
    "POST /api/auth/login": () => Response.json({ user, ...tokenPair(1) }),
    "POST /api/auth/refresh": () => Response.json(tokenPair(2)),
    "GET /api/auth/me": (call) =>
      call.headers.get("authorization") === "Bearer AT1"
        ? Response.json({ ...user, workspace: call.headers.get("x-workspace") })
        : Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 }),
    "GET /api/v1/deals": (call) =>
      Response.json({ auth: call.headers.get("authorization"), workspace: call.headers.get("x-workspace") }),
  });
}

describe("route handlers", () => {
  it("serve sign-in and proxy the rest, for every method", async () => {
    const navree = api();
    const app = createNavree({ apiUrl: API, fetch: navree.fetch });
    const { GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS } = app.handlers;
    expect([PUT, PATCH, DELETE, HEAD, OPTIONS].every((h) => typeof h === "function")).toBe(true);

    const login = await POST(browserRequest(APP, "/api/auth/login", { method: "POST", body: { email: "a", password: "b" } }));
    expect(await login.json()).toEqual({ user });
    expect(readSetCookies(login)[ACCESS_COOKIE]!.value).toBe("AT1");

    const deals = await GET(browserRequest(APP, "/api/v1/deals", { cookies: { [ACCESS_COOKIE]: "AT1" } }));
    expect(await deals.json()).toEqual({ auth: "Bearer AT1", workspace: null });
  });

  it("read NAVREE_* settings when the first request arrives", async () => {
    const navree = fakeNavree({ "GET /api/v1/deals": () => Response.json({ ok: true }) });
    const app = createNavree({ fetch: navree.fetch, env: { NAVREE_API_URL: "http://api.internal:9000" } });
    await app.handlers.GET(browserRequest(APP, "/api/v1/deals"));
    expect(navree.calls[0]!.url).toBe("http://api.internal:9000/api/v1/deals");
  });

  it("share one BFF, so concurrent requests share one refresh", async () => {
    let refreshes = 0;
    const navree = fakeNavree({
      "POST /api/auth/refresh": async () => {
        refreshes += 1;
        await new Promise((r) => setTimeout(r, 5));
        return Response.json(tokenPair(2));
      },
      "GET /api/v1/deals": () => Response.json({ ok: true }),
    });
    const app = createNavree({ apiUrl: API, fetch: navree.fetch });
    const cookies = { [REFRESH_COOKIE]: "RT1" };
    await Promise.all([
      app.handlers.GET(browserRequest(APP, "/api/v1/deals", { cookies })),
      app.handlers.GET(browserRequest(APP, "/api/v1/deals", { cookies })),
    ]);
    expect(refreshes).toBe(1);
  });
});

describe("auth() in server components", () => {
  it("returns the signed-in user, the token and the workspace", async () => {
    jar.cookies.set(ACCESS_COOKIE, "AT1");
    jar.cookies.set(WORKSPACE_COOKIE, "acme");
    const app = createNavree({ apiUrl: API, fetch: api().fetch });

    const session = await app.auth();

    expect(session).toEqual({ user: { ...user, workspace: "acme" }, accessToken: "AT1", workspace: "acme" });
  });

  it("returns null without a session or when the API refuses the token", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch });
    expect(await app.auth()).toBeNull();
    jar.cookies.set(ACCESS_COOKIE, "AT0");
    expect(await app.auth()).toBeNull();
  });

  it("client() calls the API from the server with the session's token", async () => {
    jar.cookies.set(ACCESS_COOKIE, "AT1");
    jar.cookies.set(WORKSPACE_COOKIE, "acme");
    const navree = api();
    const app = createNavree({ apiUrl: API, fetch: navree.fetch });

    type Paths = { "/api/v1/deals": { get: { parameters: { query?: never }; responses: { 200: { content: { "application/json": { auth: string; workspace: string } } } } } } };
    const client = await app.client<Paths>();
    const body = await unwrap(client.GET("/api/v1/deals"));

    expect(body).toEqual({ auth: "Bearer AT1", workspace: "acme" });
  });
});

describe("the proxy (Next.js middleware)", () => {
  const page = (path: string, cookies: Record<string, string> = {}, accept = "text/html,application/xhtml+xml") =>
    new NextRequest(browserRequest(APP, path, { cookies, headers: { accept } }));

  it("sends a browser without a session to sign-in, remembering where it was going", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch });
    const proxy = app.proxy({ loginPath: "/login" });

    const response = await proxy(page("/deals?stage=won"));

    expect(response.status).toBe(307);
    expect(response.headers.get("location")).toBe(`${APP}/login?next=%2Fdeals%3Fstage%3Dwon`);
  });

  it("lets public pages, the BFF routes, assets and non-page requests through", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch });
    const proxy = app.proxy({ loginPath: "/login", publicPaths: ["/", "/pricing", "/docs/*"] });

    for (const path of ["/login", "/", "/pricing", "/docs/a/b", "/api/v1/deals", "/broadcasting/auth", "/_next/static/x.js", "/logo.svg"]) {
      const response = await proxy(page(path));
      expect(response.headers.get("x-middleware-next"), path).toBe("1");
    }
    const json = await proxy(page("/deals", {}, "application/json"));
    expect(json.headers.get("x-middleware-next")).toBe("1");
  });

  it("lets a valid session through untouched", async () => {
    const navree = api();
    const app = createNavree({ apiUrl: API, fetch: navree.fetch });
    const fresh = unsignedJwt({ sub: "u1", exp: Math.floor(Date.now() / 1000) + 600 });

    const response = await app.proxy()(page("/deals", { [ACCESS_COOKIE]: fresh, [REFRESH_COOKIE]: "RT1" }));

    expect(response.headers.get("x-middleware-next")).toBe("1");
    expect(response.headers.getSetCookie()).toEqual([]);
    expect(navree.calls).toHaveLength(0);
  });

  it("refreshes an expired token before the page renders, for the browser and for this request", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch });
    const stale = unsignedJwt({ sub: "u1", exp: Math.floor(Date.now() / 1000) - 10 });

    const response = await app.proxy()(page("/deals", { [ACCESS_COOKIE]: stale, [REFRESH_COOKIE]: "RT1" }));

    expect(response.headers.get("x-middleware-next")).toBe("1");
    expect(readSetCookies(response)[ACCESS_COOKIE]!.value).toBe("AT2");
    // Server components rendering this request read the new tokens.
    expect(response.headers.get("x-middleware-request-cookie")).toContain(`${ACCESS_COOKIE}=AT2`);
    expect(response.headers.get("x-middleware-override-headers")).toContain("cookie");
  });

  it("clears an expired session and sends the browser to sign-in", async () => {
    const navree = fakeNavree({
      "POST /api/auth/refresh": () => Response.json({ statusCode: 401, message: "x", error: "Unauthorized" }, { status: 401 }),
    });
    const app = createNavree({ apiUrl: API, fetch: navree.fetch });

    const response = await app.proxy()(page("/deals", { [REFRESH_COOKIE]: "RT1" }));

    expect(response.status).toBe(307);
    expect(response.headers.get("location")).toBe(`${APP}/login?next=%2Fdeals`);
    expect(readSetCookies(response)[REFRESH_COOKIE]!.attrs).toContain("Max-Age=0");
  });

  it("sends a signed-in browser away from the sign-in page when asked to", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch });
    const response = await app.proxy({ signedInRedirect: "/deals" })(page("/login", { [ACCESS_COOKIE]: "opaque" }));
    expect(response.status).toBe(307);
    expect(response.headers.get("location")).toBe(`${APP}/deals`);
  });
});
