// @vitest-environment node
import { describe, expect, it } from "vitest";
import { ACCESS_COOKIE, createBff, REFRESH_COOKIE } from "./bff.js";
import { browserRequest, fakeNavree, readSetCookies, unsignedJwt } from "./testing.js";

const API = "http://navree.test";
const APP = "http://app.test";
const now = () => Math.floor(Date.now() / 1000);

/** A fetch that never reaches the API. */
const down = (async () => {
  throw new TypeError("fetch failed");
}) as typeof fetch;
const refused = (status: number) => () =>
  Response.json({ statusCode: status, message: "x", error: "x" }, { status });

describe("a refresh the API could not answer does not end the session", () => {
  it("ensureSession is unavailable, with no cookies to clear, when the API is down", async () => {
    const bff = createBff({ apiUrl: API, fetch: down });
    const stale = unsignedJwt({ sub: "u1", exp: now() - 60 });

    const session = await bff.ensureSession(
      browserRequest(APP, "/deals", { cookies: { [ACCESS_COOKIE]: stale, [REFRESH_COOKIE]: "RT1" } }),
    );

    expect(session).toEqual({ status: "unavailable", setCookie: [] });
  });

  it("ensureSession is unavailable when only the refresh cookie is left and the API fails", async () => {
    for (const status of [500, 502, 503, 408, 429]) {
      const api = fakeNavree({ "POST /api/auth/refresh": refused(status) });
      const bff = createBff({ apiUrl: API, fetch: api.fetch });
      const session = await bff.ensureSession(browserRequest(APP, "/", { cookies: { [REFRESH_COOKIE]: "RT1" } }));
      expect(session, String(status)).toEqual({ status: "unavailable", setCookie: [] });
    }
  });

  it("ensureSession still uses an access token that has not actually expired", async () => {
    const bff = createBff({ apiUrl: API, fetch: down });
    const nearly = unsignedJwt({ sub: "u1", exp: now() + 10 });

    const session = await bff.ensureSession(
      browserRequest(APP, "/deals", { cookies: { [ACCESS_COOKIE]: nearly, [REFRESH_COOKIE]: "RT1" } }),
    );

    expect(session).toEqual({ status: "active", accessToken: nearly, setCookie: [] });
  });

  it("a refresh the API refuses (4xx) is still the end of the session", async () => {
    for (const status of [400, 401, 403]) {
      const api = fakeNavree({ "POST /api/auth/refresh": refused(status) });
      const bff = createBff({ apiUrl: API, fetch: api.fetch });
      const session = await bff.ensureSession(browserRequest(APP, "/", { cookies: { [REFRESH_COOKIE]: "RT1" } }));
      expect(session.status, String(status)).toBe("expired");
      expect(readSetCookies(session.setCookie)[REFRESH_COOKIE]!.attrs).toContain("Max-Age=0");
    }
  });

  it("tries the refresh again on the next request instead of remembering the outage", async () => {
    let up = false;
    const api = fakeNavree({
      "POST /api/auth/refresh": () =>
        up ? Response.json({ access_token: "AT2", refresh_token: "RT2", expires_in: 900 }) : new Response(null, { status: 503 }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const request = () => browserRequest(APP, "/", { cookies: { [REFRESH_COOKIE]: "RT1" } });

    expect((await bff.ensureSession(request())).status).toBe("unavailable");
    up = true;
    expect((await bff.ensureSession(request())).status).toBe("refreshed");
  });

  it("the browser's refresh call answers 502 and keeps the cookies when the API is down", async () => {
    const bff = createBff({ apiUrl: API, fetch: down });

    const response = await bff.refreshSession(
      browserRequest(APP, "/api/auth/refresh", { method: "POST", cookies: { [REFRESH_COOKIE]: "RT1" } }),
    );

    expect(response.status).toBe(502);
    expect(await response.json()).toMatchObject({ code: "api_unreachable" });
    expect(response.headers.getSetCookie()).toEqual([]);
  });

  it("a proxied call answers 502 and keeps the cookies when the API can't be asked to refresh", async () => {
    const api = fakeNavree({
      "GET /api/v1/deals": refused(401),
      "POST /api/auth/refresh": () => new Response(null, { status: 503 }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const expired = await bff.proxy(
      browserRequest(APP, "/api/v1/deals", { cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" } }),
      "/api/v1/deals",
    );
    expect(expired.status).toBe(502);
    expect(expired.headers.getSetCookie()).toEqual([]);

    const onlyRefresh = await bff.proxy(
      browserRequest(APP, "/api/v1/deals", { cookies: { [REFRESH_COOKIE]: "RT1" } }),
      "/api/v1/deals",
    );
    expect(onlyRefresh.status).toBe(502);
    expect(onlyRefresh.headers.getSetCookie()).toEqual([]);
  });
});

describe("server-side calls name the browser", () => {
  it("bff.api sends the client address a trusted proxy recorded, when given the request", async () => {
    const api = fakeNavree({ "GET /api/v1/deals": () => Response.json({}) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, trustedProxyHops: 1 });
    const request = browserRequest(APP, "/deals", { headers: { "x-forwarded-for": "6.6.6.6, 203.0.113.9" } });

    await bff.api("/api/v1/deals", { accessToken: "AT1", request });
    await bff.api("/api/v1/deals", { accessToken: "AT1" });

    expect(api.calls[0]!.headers.get("x-forwarded-for")).toBe("203.0.113.9");
    expect(api.calls[1]!.headers.get("x-forwarded-for")).toBeNull();
  });
});

describe("pages that the API redirects or refuses", () => {
  it("keeps a redirect to the API's own address on the app's origin", async () => {
    const api = fakeNavree({
      "GET /admin": () => new Response(null, { status: 302, headers: { location: `${API}/admin/users?page=2` } }),
      "GET /admin/other": () => new Response(null, { status: 302, headers: { location: "https://elsewhere.example/x" } }),
      "GET /admin/path": () => new Response(null, { status: 302, headers: { location: "/admin/users" } }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const cookies = { [ACCESS_COOKIE]: "AT1" };

    expect((await bff.handle(browserRequest(APP, "/admin", { cookies }))).headers.get("location")).toBe("/admin/users?page=2");
    expect((await bff.handle(browserRequest(APP, "/admin/other", { cookies }))).headers.get("location")).toBe(
      "https://elsewhere.example/x",
    );
    expect((await bff.handle(browserRequest(APP, "/admin/path", { cookies }))).headers.get("location")).toBe("/admin/users");
  });

  it("does not take an address that only begins like the API's for the API", async () => {
    const api = fakeNavree({
      "GET /admin": () => new Response(null, { status: 302, headers: { location: `${API}.evil.example/x` } }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const response = await bff.handle(browserRequest(APP, "/admin", { cookies: { [ACCESS_COOKIE]: "AT1" } }));
    expect(response.headers.get("location")).toBe(`${API}.evil.example/x`);
  });

  it("sends a page request that loses its session to the sign-in page when asked to, and clears the cookies", async () => {
    const api = fakeNavree({ "GET /admin/users": refused(401) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, redirectNavigations: true, pages: { signIn: "/sign-in" } });

    const response = await bff.handle(
      browserRequest(APP, "/admin/users?page=2", {
        cookies: { [ACCESS_COOKIE]: "AT1" },
        headers: { accept: "text/html,application/xhtml+xml" },
      }),
    );

    expect(response.status).toBe(303);
    expect(response.headers.get("location")).toBe("/sign-in?next=%2Fadmin%2Fusers%3Fpage%3D2");
    expect(readSetCookies(response)[ACCESS_COOKIE]!.attrs).toContain("Max-Age=0");
  });

  it("passes the 401 on for a call that does not ask for HTML, and when the option is off", async () => {
    const api = fakeNavree({ "GET /admin/users": refused(401) });
    const on = createBff({ apiUrl: API, fetch: api.fetch, redirectNavigations: true });
    const off = createBff({ apiUrl: API, fetch: api.fetch });
    const page = { cookies: { [ACCESS_COOKIE]: "AT1" }, headers: { accept: "text/html" } };

    expect((await on.handle(browserRequest(APP, "/admin/users", { cookies: page.cookies }))).status).toBe(401);
    expect((await on.handle(browserRequest(APP, "/admin/users", { ...page, headers: { accept: "application/json" } }))).status).toBe(401);
    expect((await off.handle(browserRequest(APP, "/admin/users", page))).status).toBe(401);
  });
});
