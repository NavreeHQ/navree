// @vitest-environment node
import { describe, expect, it } from "vitest";
import { ACCESS_COOKIE, CHALLENGE_COOKIE, createBff, REFRESH_COOKIE, SOCIAL_COOKIE } from "./bff.js";
import { browserRequest, fakeNavree, readSetCookies, tokenPair } from "./testing.js";

const API = "http://navree.test";
const APP = "http://app.test";

const get = (path: string, cookies: Record<string, string> = {}, headers: Record<string, string> = {}) =>
  browserRequest(APP, path, { cookies, headers });

const authorize = { url: "https://github.com/login/oauth/authorize?client_id=x&state=S1", state: "S1" };

describe("social sign-in: the start", () => {
  it("asks the API for the provider's URL, keeps the state in a cookie only the callback route receives, and redirects", async () => {
    const api = fakeNavree({ "GET /api/auth/social/github/redirect": () => Response.json(authorize) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, secureCookies: true });

    const response = await bff.socialRedirect(get("/auth/social/github?next=/w/agra"), "github");

    expect(response.status).toBe(303);
    expect(response.headers.get("location")).toBe(authorize.url);
    expect(new URL(api.calls[0]!.url).search).toBe("?return_to=%2Fw%2Fagra");
    const cookies = readSetCookies(response);
    expect(cookies[SOCIAL_COOKIE]!.value).toBe("S1");
    expect(cookies[SOCIAL_COOKIE]!.attrs).toEqual(
      expect.arrayContaining(["Path=/auth/callback", "HttpOnly", "SameSite=Lax", "Max-Age=600", "Secure"]),
    );
  });

  it("only ever returns to a path on this origin", async () => {
    const api = fakeNavree({ "GET /api/auth/social/github/redirect": () => Response.json(authorize) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    for (const next of ["//evil.example/x", "https://evil.example", "/\\evil.example", "w/agra"]) {
      await bff.socialRedirect(get(`/auth/social/github?next=${encodeURIComponent(next)}`), "github");
    }
    await bff.socialRedirect(get("/auth/social/github"), "github");

    expect(api.calls.map((call) => new URL(call.url).searchParams.get("return_to"))).toEqual(["/", "/", "/", "/", "/"]);
  });

  it("sends the browser to the sign-in page with an error when the provider is unavailable or the API is down", async () => {
    const refused = fakeNavree({
      "GET /api/auth/social/github/redirect": () =>
        Response.json({ statusCode: 404, message: "Not Found", error: "Not Found" }, { status: 404 }),
    });
    const bff = createBff({ apiUrl: API, fetch: refused.fetch, pages: { signIn: "/sign-in" } });
    const unavailable = await bff.socialRedirect(get("/auth/social/github"), "github");
    expect(unavailable.status).toBe(303);
    expect(unavailable.headers.get("location")).toBe("/sign-in?error=social_unavailable");
    expect(unavailable.headers.getSetCookie()).toEqual([]);

    const down = createBff({
      apiUrl: API,
      fetch: (async () => {
        throw new TypeError("fetch failed");
      }) as typeof fetch,
    });
    const unreachable = await down.socialRedirect(get("/auth/social/github"), "github");
    expect(unreachable.headers.get("location")).toBe("/login?error=api_unreachable");
  });

  it("does not follow an answer that is not a web address", async () => {
    const api = fakeNavree({
      "GET /api/auth/social/github/redirect": () => Response.json({ url: "javascript:alert(1)", state: "S1" }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const response = await bff.socialRedirect(get("/auth/social/github"), "github");
    expect(response.headers.get("location")).toBe("/login?error=social_unavailable");
  });

  it("names the browser to the API when a trusted proxy recorded it, and never takes the browser's own word", async () => {
    const api = fakeNavree({ "GET /api/auth/social/github/redirect": () => Response.json(authorize) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, trustedProxyHops: 1 });

    await bff.socialRedirect(get("/auth/social/github", {}, { "x-forwarded-for": "6.6.6.6, 203.0.113.9" }), "github");

    expect(api.calls[0]!.headers.get("x-forwarded-for")).toBe("203.0.113.9");

    const open = createBff({ apiUrl: API, fetch: api.fetch });
    await open.socialRedirect(get("/auth/social/github", {}, { "x-forwarded-for": "6.6.6.6" }), "github");
    expect(api.calls[1]!.headers.get("x-forwarded-for")).toBeNull();
  });

  it("puts the provider in the path as one encoded segment", async () => {
    const api = fakeNavree({ "GET /api/auth/social/a%2Fb/redirect": () => Response.json(authorize) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const response = await bff.socialRedirect(get("/auth/social/x"), "a/b");
    expect(response.headers.get("location")).toBe(authorize.url);
  });
});

describe("social sign-in: the callback", () => {
  const callback = (path = "/auth/callback/github?code=C1&state=S1", cookies: Record<string, string> = { [SOCIAL_COOKIE]: "S1" }) =>
    get(path, cookies);

  it("exchanges the code, stores the tokens like a password sign-in and lands on the page the sign-in started from", async () => {
    const api = fakeNavree({
      "POST /api/auth/social/github/callback": () => Response.json({ ...tokenPair(1), return_to: "/w/agra" }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, secureCookies: false });

    const response = await bff.socialCallback(callback(), "github");

    expect(response.status).toBe(303);
    expect(response.headers.get("location")).toBe("/w/agra");
    expect(JSON.parse(api.calls[0]!.body)).toEqual({ code: "C1", state: "S1" });
    const cookies = readSetCookies(response);
    expect(cookies[ACCESS_COOKIE]).toMatchObject({ value: "AT1" });
    expect(cookies[ACCESS_COOKIE]!.attrs).toEqual(expect.arrayContaining(["HttpOnly", "Max-Age=900", "Path=/"]));
    expect(cookies[REFRESH_COOKIE]).toMatchObject({ value: "RT1" });
    expect(cookies[REFRESH_COOKIE]!.attrs).toContain(`Max-Age=${60 * 60 * 24 * 30}`);
    // The state is spent.
    expect(cookies[SOCIAL_COOKIE]!.attrs).toEqual(expect.arrayContaining(["Max-Age=0", "Path=/auth/callback"]));
  });

  it("never returns to another origin, whatever the API says", async () => {
    const api = fakeNavree({
      "POST /api/auth/social/github/callback": () => Response.json({ ...tokenPair(1), return_to: "//evil.example" }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const response = await bff.socialCallback(callback(), "github");
    expect(response.headers.get("location")).toBe("/");
  });

  it("continues at the two-factor page for an account with two-factor on, with the challenge in its own cookie", async () => {
    const api = fakeNavree({
      "POST /api/auth/social/github/callback": () =>
        Response.json({ two_factor_required: true, challenge_token: "CHALLENGE", expires_in: 300, return_to: "/w/agra" }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const response = await bff.socialCallback(callback(), "github");

    expect(response.headers.get("location")).toBe("/two-factor?next=%2Fw%2Fagra");
    const cookies = readSetCookies(response);
    expect(cookies[CHALLENGE_COOKIE]).toMatchObject({ value: "CHALLENGE" });
    expect(cookies[CHALLENGE_COOKIE]!.attrs).toEqual(expect.arrayContaining(["Path=/api/auth/2fa", "Max-Age=300", "HttpOnly"]));
    expect(cookies[ACCESS_COOKIE]).toBeUndefined();
  });

  it("refuses a state that does not match the cookie without calling the API, and spends the cookie", async () => {
    const api = fakeNavree({});
    const bff = createBff({ apiUrl: API, fetch: api.fetch, pages: { signIn: "/sign-in" } });

    for (const request of [
      callback("/auth/callback/github?code=C1&state=OTHER"),
      callback("/auth/callback/github?code=C1&state=S1", {}),
      callback("/auth/callback/github?state=S1"),
      callback("/auth/callback/github?code=C1"),
    ]) {
      const response = await bff.socialCallback(request, "github");
      expect(response.headers.get("location")).toBe("/sign-in?error=social_state");
      expect(readSetCookies(response)[SOCIAL_COOKIE]!.attrs).toContain("Max-Age=0");
    }
    expect(api.calls).toHaveLength(0);
  });

  it("ends on the sign-in page when the API refuses the code, answers with something else, or is down", async () => {
    const refused = fakeNavree({
      "POST /api/auth/social/github/callback": () =>
        Response.json({ statusCode: 401, message: "x", error: "Unauthorized" }, { status: 401 }),
    });
    expect((await createBff({ apiUrl: API, fetch: refused.fetch }).socialCallback(callback(), "github")).headers.get("location")).toBe(
      "/login?error=social_failed",
    );

    const odd = fakeNavree({ "POST /api/auth/social/github/callback": () => Response.json({ hello: "world" }) });
    const oddResponse = await createBff({ apiUrl: API, fetch: odd.fetch }).socialCallback(callback(), "github");
    expect(oddResponse.headers.get("location")).toBe("/login?error=social_failed");
    expect(readSetCookies(oddResponse)[ACCESS_COOKIE]).toBeUndefined();

    const down = createBff({
      apiUrl: API,
      fetch: (async () => {
        throw new TypeError("fetch failed");
      }) as typeof fetch,
    });
    expect((await down.socialCallback(callback(), "github")).headers.get("location")).toBe("/login?error=api_unreachable");
  });

  it("uses the cookie names and pages the app chose", async () => {
    const api = fakeNavree({ "POST /api/auth/social/github/callback": () => Response.json(tokenPair(2)) });
    const bff = createBff({
      apiUrl: API,
      fetch: api.fetch,
      cookieNames: { access: "app_at", refresh: "app_rt", social: "app_social" },
    });
    const response = await bff.socialCallback(callback("/auth/callback/github?code=C1&state=S1", { app_social: "S1" }), "github");
    const cookies = readSetCookies(response);
    expect(Object.keys(cookies).sort()).toEqual(["app_at", "app_rt", "app_social"]);
  });
});

describe("the verification link", () => {
  const link = "/api/auth/email/verify/u1/abc123?expires=1900000000&signature=SIG";

  it("forwards the signed link to the API and lands on the verify-email page", async () => {
    const api = fakeNavree({ "GET /api/auth/email/verify/u1/abc123": () => new Response(null, { status: 204 }) });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, pages: { verifyEmail: "/check-your-email" } });

    const response = await bff.verifyEmail(get(link));

    expect(response.status).toBe(303);
    expect(response.headers.get("location")).toBe("/check-your-email?status=verified");
    expect(api.calls[0]!.url).toBe(`${API}${link}`);
    expect(api.calls[0]!.headers.get("authorization")).toBeNull();
  });

  it("says invalid when the API refuses the link, unreachable when it fails or is down", async () => {
    const refused = fakeNavree({
      "GET /api/auth/email/verify/u1/abc123": () =>
        Response.json({ statusCode: 403, message: "x", error: "Forbidden" }, { status: 403 }),
    });
    expect((await createBff({ apiUrl: API, fetch: refused.fetch }).verifyEmail(get(link))).headers.get("location")).toBe(
      "/verify-email?status=invalid",
    );

    const failing = fakeNavree({
      "GET /api/auth/email/verify/u1/abc123": () => new Response(null, { status: 503 }),
    });
    expect((await createBff({ apiUrl: API, fetch: failing.fetch }).verifyEmail(get(link))).headers.get("location")).toBe(
      "/verify-email?status=unreachable",
    );

    const down = createBff({
      apiUrl: API,
      fetch: (async () => {
        throw new TypeError("fetch failed");
      }) as typeof fetch,
    });
    expect((await down.verifyEmail(get(link))).headers.get("location")).toBe("/verify-email?status=unreachable");
  });

  it("answers only the verification link's path", async () => {
    const api = fakeNavree({});
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    for (const path of ["/api/auth/me", "/api/auth/email/verify/u1", "/api/auth/email/verify/u1/a/b", "/api/auth/email/verify//x"]) {
      const response = await bff.verifyEmail(get(path));
      expect(response.status).toBe(404);
    }
    expect(api.calls).toHaveLength(0);
  });
});

describe("bff.handle routes the GET pages of social sign-in and the verification link", () => {
  it("starts, finishes and verifies on the paths the app mounts", async () => {
    const api = fakeNavree({
      "GET /api/auth/social/github/redirect": () => Response.json(authorize),
      "POST /api/auth/social/github/callback": () => Response.json(tokenPair(3)),
      "GET /api/auth/email/verify/u1/h1": () => new Response(null, { status: 204 }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch });

    const start = await bff.handle(get("/auth/social/github?next=/"));
    expect(start.headers.get("location")).toBe(authorize.url);

    const done = await bff.handle(get("/auth/callback/github?code=C&state=S1", { [SOCIAL_COOKIE]: "S1" }));
    expect(done.headers.get("location")).toBe("/");
    expect(readSetCookies(done)[ACCESS_COOKIE]!.value).toBe("AT3");

    const verified = await bff.handle(get("/api/auth/email/verify/u1/h1?signature=s"));
    expect(verified.headers.get("location")).toBe("/verify-email?status=verified");
  });

  it("uses the paths the app chose and leaves every other GET to the proxy", async () => {
    const api = fakeNavree({
      "GET /api/auth/social/github/redirect": () => Response.json(authorize),
      "GET /auth/social/github/extra": () => Response.json({ proxied: true }),
    });
    const bff = createBff({ apiUrl: API, fetch: api.fetch, socialPaths: { start: "/oauth/start", callback: "/oauth/back" } });

    const start = await bff.handle(get("/oauth/start/github"));
    expect(start.headers.get("location")).toBe(authorize.url);
    expect(readSetCookies(start)[SOCIAL_COOKIE]!.attrs).toContain("Path=/oauth/back");

    // Not the app's social path any more, so it is an ordinary call.
    const other = await bff.handle(get("/auth/social/github/extra"));
    expect(await other.json()).toEqual({ proxied: true });
  });

  it("does not treat a POST to a social path as the sign-in", async () => {
    const api = fakeNavree({});
    const bff = createBff({ apiUrl: API, fetch: api.fetch });
    const response = await bff.handle(browserRequest(APP, "/auth/social/github", { method: "POST", body: {} }));
    expect(response.status).toBe(404);
    expect(api.calls[0]!.method).toBe("POST");
  });
});
