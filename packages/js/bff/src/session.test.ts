import { describe, expect, it } from "vitest";
import { ACCESS_COOKIE, createBff, REFRESH_COOKIE, WORKSPACE_COOKIE, tokenExpiresAt } from "./bff.js";
import { browserRequest, fakeNavree, readSetCookies, tokenPair, unsignedJwt } from "./testing.js";

const API = "http://navree.test";
const APP = "http://app.test";
const user = { __type: "User", id: "u1", name: "Anu" };
const now = () => Math.floor(Date.now() / 1000);

describe("tokenExpiresAt", () => {
  it("reads a JWT's exp without verifying it, and nothing from an opaque token", () => {
    expect(tokenExpiresAt(unsignedJwt({ sub: "u1", exp: 1_900_000_000 }))).toBe(1_900_000_000);
    expect(tokenExpiresAt(unsignedJwt({ sub: "u1" }))).toBeUndefined();
    expect(tokenExpiresAt("opaque-token")).toBeUndefined();
    expect(tokenExpiresAt("a.!!!.c")).toBeUndefined();
  });
});

describe("bff.ensureSession (middleware and server rendering)", () => {
  it("is anonymous without token cookies", async () => {
    const bff = createBff({ apiUrl: API, fetch: fakeNavree({}).fetch });
    const session = await bff.ensureSession(browserRequest(APP, "/deals"));
    expect(session).toEqual({ status: "anonymous", setCookie: [] });
  });

  it("is active with an access token that has not expired (or can't be read)", async () => {
    const api = fakeNavree({});
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const fresh = unsignedJwt({ sub: "u1", exp: now() + 600 });

    const session = await bff.ensureSession(browserRequest(APP, "/deals", { cookies: { [ACCESS_COOKIE]: fresh, [REFRESH_COOKIE]: "RT1" } }));
    expect(session).toEqual({ status: "active", accessToken: fresh, setCookie: [] });

    const opaque = await bff.ensureSession(browserRequest(APP, "/deals", { cookies: { [ACCESS_COOKIE]: "opaque" } }));
    expect(opaque.status).toBe("active");
    expect(api.calls).toHaveLength(0);
  });

  it("refreshes an access token that expired (or is about to), and hands back the new cookies", async () => {
    const api = fakeNavree({ "POST /api/auth/refresh": () => Response.json(tokenPair(2)) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const stale = unsignedJwt({ sub: "u1", exp: now() + 5 });

    const session = await bff.ensureSession(
      browserRequest(APP, "/deals", { cookies: { [ACCESS_COOKIE]: stale, [REFRESH_COOKIE]: "RT1", theme: "dark" } }),
    );

    expect(session.status).toBe("refreshed");
    expect(session.accessToken).toBe("AT2");
    const cookies = readSetCookies(session.setCookie);
    expect(cookies[ACCESS_COOKIE]!.value).toBe("AT2");
    expect(cookies[REFRESH_COOKIE]!.value).toBe("RT2");
    // The request's Cookie header with the new tokens, for the page rendering this same request.
    expect(session.cookieHeader).toBe(`${ACCESS_COOKIE}=AT2; ${REFRESH_COOKIE}=RT2; theme=dark`);
    expect(JSON.parse(api.calls[0]!.body)).toEqual({ refresh_token: "RT1" });
  });

  it("refreshes when only the refresh cookie is left", async () => {
    const api = fakeNavree({ "POST /api/auth/refresh": () => Response.json(tokenPair(3)) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const session = await bff.ensureSession(browserRequest(APP, "/", { cookies: { [REFRESH_COOKIE]: "RT2" } }));
    expect(session.status).toBe("refreshed");
    expect(session.cookieHeader).toBe(`${REFRESH_COOKIE}=RT3; ${ACCESS_COOKIE}=AT3`);
  });

  it("is expired, with cookies that clear the session, when the refresh fails", async () => {
    const api = fakeNavree({
      "POST /api/auth/refresh": () => Response.json({ statusCode: 401, message: "x", error: "Unauthorized" }, { status: 401 }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const session = await bff.ensureSession(browserRequest(APP, "/", { cookies: { [REFRESH_COOKIE]: "RT9" } }));
    expect(session.status).toBe("expired");
    expect(session.accessToken).toBeUndefined();
    expect(readSetCookies(session.setCookie)[REFRESH_COOKIE]!.attrs).toContain("Max-Age=0");
  });

  it("shares the refresh with a proxied request spending the same refresh token", async () => {
    let refreshes = 0;
    const api = fakeNavree({
      "POST /api/auth/refresh": async () => {
        refreshes += 1;
        await new Promise((r) => setTimeout(r, 5));
        return Response.json(tokenPair(4));
      },
      "GET /api/auth/me": () => Response.json(user),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const cookies = { [REFRESH_COOKIE]: "RT3" };
    await Promise.all([
      bff.ensureSession(browserRequest(APP, "/", { cookies })),
      bff.proxy(browserRequest(APP, "/api/auth/me", { cookies }), "/api/auth/me"),
    ]);
    expect(refreshes).toBe(1);
  });
});

describe("server-side calls with the session's token", () => {
  it("bff.api calls Navree with the bearer token and the workspace", async () => {
    const api = fakeNavree({
      "GET /api/v1/deals": (call) =>
        Response.json({ auth: call.headers.get("authorization"), ws: call.headers.get("x-workspace") }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const response = await bff.api("/api/v1/deals?page=2", { accessToken: "AT1", workspace: "agra" });

    expect(await response.json()).toEqual({ auth: "Bearer AT1", ws: "agra" });
    expect(api.calls[0]!.url).toBe(`${API}/api/v1/deals?page=2`);
  });

  it("bff.me returns the signed-in user, or null when the token is refused", async () => {
    const api = fakeNavree({
      "GET /api/auth/me": (call) =>
        call.headers.get("authorization") === "Bearer AT1"
          ? Response.json(user)
          : Response.json({ statusCode: 401, message: "x", error: "Unauthorized" }, { status: 401 }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    expect(await bff.me("AT1")).toEqual(user);
    expect(await bff.me("AT0")).toBeNull();
    expect(await bff.me(undefined)).toBeNull();
  });

  it("bff.me throws when the API is down, so a page can tell 'signed out' from 'unreachable'", async () => {
    const bff = createBff({
      apiUrl: API,
      fetch: (async () => {
        throw new TypeError("fetch failed");
      }) as typeof fetch,
    });
    await expect(bff.me("AT1")).rejects.toThrow(TypeError);
  });

  it("reads the workspace cookie of a request", () => {
    const bff = createBff({ apiUrl: API, fetch: fakeNavree({}).fetch });
    expect(bff.workspaceOf(browserRequest(APP, "/", { cookies: { [WORKSPACE_COOKIE]: "agra" } }))).toBe("agra");
    expect(bff.workspaceOf(browserRequest(APP, "/"))).toBeUndefined();
  });
});
