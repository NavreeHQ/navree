// @vitest-environment node
import { ACCESS_COOKIE, REFRESH_COOKIE, WORKSPACE_COOKIE } from "@navree/bff";
import { browserRequest, fakeNavree, readSetCookies, tokenPair, unsignedJwt } from "@navree/bff/testing";
import { unwrap } from "@navree/client";
import { NextRequest } from "next/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createNavree } from "./navree.js";

const API = "http://navree.test";
const APP = "http://app.test";
const user = { __type: "User", id: "u1", name: "Anu" };

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

describe("social sign-in and the verification link", () => {
  it("are answered by the same handlers, one catch-all route per mount", async () => {
    const navree = fakeNavree({
      "GET /api/auth/social/github/redirect": () => Response.json({ url: "https://github.com/login/oauth/authorize?state=S1", state: "S1" }),
      "POST /api/auth/social/github/callback": () => Response.json({ ...tokenPair(1), return_to: "/w/agra" }),
      "GET /api/auth/email/verify/u1/h1": () => new Response(null, { status: 204 }),
    });
    const app = createNavree({ apiUrl: API, fetch: navree.fetch, pages: { signIn: "/sign-in" } });

    const start = await app.handlers.GET(browserRequest(APP, "/auth/social/github?next=/w/agra"));
    expect(start.status).toBe(303);
    expect(start.headers.get("location")).toBe("https://github.com/login/oauth/authorize?state=S1");

    const done = await app.handlers.GET(browserRequest(APP, "/auth/callback/github?code=C&state=S1", { cookies: { navree_social: "S1" } }));
    expect(done.headers.get("location")).toBe("/w/agra");
    expect(readSetCookies(done)[ACCESS_COOKIE]!.value).toBe("AT1");

    const verified = await app.handlers.GET(browserRequest(APP, "/api/auth/email/verify/u1/h1?signature=s"));
    expect(verified.headers.get("location")).toBe("/verify-email?status=verified");
  });

  it("the server entry point exports readCookie for route handlers", async () => {
    const server = await import("./index.js");
    expect(server.readCookie(browserRequest(APP, "/", { cookies: { [WORKSPACE_COOKIE]: "agra" } }), WORKSPACE_COOKIE)).toBe("agra");
  });
});

describe("auth() in server components", () => {
  it("returns the signed-in user, the token and the workspace", async () => {
    jar.cookies.set(ACCESS_COOKIE, "AT1");
    jar.cookies.set(WORKSPACE_COOKIE, "agra");
    const app = createNavree({ apiUrl: API, fetch: api().fetch });

    const session = await app.auth();

    expect(session).toEqual({ user: { ...user, workspace: "agra" }, accessToken: "AT1", workspace: "agra" });
  });

  it("returns null without a session or when the API refuses the token", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch });
    expect(await app.auth()).toBeNull();
    jar.cookies.set(ACCESS_COOKIE, "AT0");
    expect(await app.auth()).toBeNull();
  });

  it("client() calls the API from the server with the session's token", async () => {
    jar.cookies.set(ACCESS_COOKIE, "AT1");
    jar.cookies.set(WORKSPACE_COOKIE, "agra");
    const navree = api();
    const app = createNavree({ apiUrl: API, fetch: navree.fetch });

    type Paths = { "/api/v1/deals": { get: { parameters: { query?: never }; responses: { 200: { content: { "application/json": { auth: string; workspace: string } } } } } } };
    const client = await app.client<Paths>();
    const body = await unwrap(client.GET("/api/v1/deals"));

    expect(body).toEqual({ auth: "Bearer AT1", workspace: "agra" });
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

  it("serves the page and keeps the cookies when the API can't be reached to refresh the token", async () => {
    const down = (async () => {
      throw new TypeError("fetch failed");
    }) as typeof fetch;
    const app = createNavree({ apiUrl: API, fetch: down });
    const stale = unsignedJwt({ sub: "u1", exp: Math.floor(Date.now() / 1000) - 10 });

    const response = await app.proxy()(page("/deals", { [ACCESS_COOKIE]: stale, [REFRESH_COOKIE]: "RT1" }));

    expect(response.status).toBe(200);
    expect(response.headers.get("x-middleware-next")).toBe("1");
    expect(response.headers.getSetCookie()).toEqual([]);
  });

  it("takes the sign-in page from the BFF settings, and lets the social sign-in routes through", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch, pages: { signIn: "/sign-in" } });
    const proxy = app.proxy();

    const guarded = await proxy(page("/deals"));
    expect(guarded.headers.get("location")).toBe(`${APP}/sign-in?next=%2Fdeals`);

    for (const path of ["/sign-in", "/auth/social/github", "/auth/callback/github?code=c&state=s"]) {
      const response = await proxy(page(path));
      expect(response.headers.get("x-middleware-next"), path).toBe("1");
    }

    const moved = createNavree({ apiUrl: API, fetch: api().fetch, socialPaths: { start: "/oauth/go", callback: "/oauth/back" } });
    for (const path of ["/oauth/go/github", "/oauth/back/github"]) {
      const response = await moved.proxy()(page(path));
      expect(response.headers.get("x-middleware-next"), path).toBe("1");
    }
    expect((await moved.proxy()(page("/auth/social/github"))).status).toBe(307);
  });

  it("sends a signed-in browser away from the sign-in page when asked to", async () => {
    const app = createNavree({ apiUrl: API, fetch: api().fetch });
    const response = await app.proxy({ signedInRedirect: "/deals" })(page("/login", { [ACCESS_COOKIE]: "opaque" }));
    expect(response.status).toBe(307);
    expect(response.headers.get("location")).toBe(`${APP}/deals`);
  });
});
