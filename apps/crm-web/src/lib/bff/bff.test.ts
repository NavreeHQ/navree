// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
import { ACCESS_COOKIE, CHALLENGE_COOKIE, createBff, REFRESH_COOKIE } from "./bff";

const API = "http://navis.test";
const APP = "http://crm.test";

type Call = { url: string; method: string; headers: Headers; body: string };

/** A scripted Navis API: routes by "METHOD /path", records every call. */
function fakeNavis(routes: Record<string, (call: Call) => Response | Promise<Response>>) {
  const calls: Call[] = [];
  const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = (init?.method ?? "GET").toUpperCase();
    const raw = init?.body;
    const body =
      raw === undefined || raw === null
        ? ""
        : typeof raw === "string"
          ? raw
          : new TextDecoder().decode(raw as ArrayBuffer | Uint8Array);
    const call: Call = { url, method, headers: new Headers(init?.headers), body };
    calls.push(call);
    const route = routes[`${method} ${new URL(url).pathname}`];
    if (!route) return Response.json({ statusCode: 404, message: "Not Found", error: "Not Found" }, { status: 404 });
    return route(call);
  });
  return { fetch: fetch as unknown as typeof globalThis.fetch, calls };
}

function request(
  path: string,
  {
    method = "GET",
    cookies = {},
    origin,
    body,
    headers = {},
  }: {
    method?: string;
    cookies?: Record<string, string>;
    origin?: string | null;
    body?: unknown;
    headers?: Record<string, string>;
  } = {},
): Request {
  const h = new Headers({ host: new URL(APP).host, ...headers });
  const cookieHeader = Object.entries(cookies)
    .map(([k, v]) => `${k}=${v}`)
    .join("; ");
  if (cookieHeader) h.set("cookie", cookieHeader);
  if (origin !== null) h.set("origin", origin ?? (method === "GET" ? "" : APP));
  if (h.get("origin") === "") h.delete("origin");
  if (body !== undefined) h.set("content-type", "application/json");
  return new Request(`${APP}${path}`, {
    method,
    headers: h,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

/** Parses Set-Cookie headers into {name: {value, attrs}}. */
function setCookies(response: Response) {
  const out: Record<string, { value: string; attrs: string[] }> = {};
  for (const line of response.headers.getSetCookie()) {
    const [pair, ...attrs] = line.split("; ");
    const eq = pair.indexOf("=");
    out[pair.slice(0, eq)] = { value: decodeURIComponent(pair.slice(eq + 1)), attrs };
  }
  return out;
}

const tokens = (n: number) => ({
  access_token: `AT${n}`,
  refresh_token: `RT${n}`,
  token_type: "Bearer",
  expires_in: 900,
});

const user = { __type: "User", id: "u1", name: "Priya Shah", email: "rep1@navis.test", roles: ["rep"] };

describe("BFF proxy", () => {
  it("forwards to Navis with the access token as a bearer header, never the cookies", async () => {
    const navis = fakeNavis({
      "GET /api/v1/deals": () =>
        Response.json({ data: [], meta: { total: 0 } }, { headers: { "x-request-id": "req-1", etag: '"v1"' } }),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.proxy(
      request("/api/v1/deals?owner_id=u1", { cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" } }),
      "/api/v1/deals",
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ data: [], meta: { total: 0 } });
    const [call] = navis.calls;
    expect(call.url).toBe(`${API}/api/v1/deals?owner_id=u1`);
    expect(call.headers.get("authorization")).toBe("Bearer AT1");
    expect(call.headers.get("cookie")).toBeNull();
    expect(response.headers.get("x-request-id")).toBe("req-1");
    expect(response.headers.get("etag")).toBe('"v1"');
    expect(response.headers.getSetCookie()).toEqual([]);
  });

  it("passes JSON bodies through and streams non-JSON responses (CSV export) with their headers", async () => {
    const navis = fakeNavis({
      "POST /api/v1/companies": (call) =>
        Response.json({ __type: "Company", ...JSON.parse(call.body) }, { status: 201 }),
      "GET /api/v1/contacts/export": () =>
        new Response("id,first_name\r\n1,Elena\r\n", {
          headers: {
            "content-type": "text/csv; charset=utf-8",
            "content-disposition": 'attachment; filename="contacts.csv"',
          },
        }),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });
    const cookies = { [ACCESS_COOKIE]: "AT1" };

    const created = await bff.proxy(
      request("/api/v1/companies", { method: "POST", cookies, body: { name: "Bluefin" } }),
      "/api/v1/companies",
    );
    const csv = await bff.proxy(request("/api/v1/contacts/export", { cookies }), "/api/v1/contacts/export");

    expect(created.status).toBe(201);
    expect(await created.json()).toEqual({ __type: "Company", name: "Bluefin" });
    expect(navis.calls[0].headers.get("content-type")).toBe("application/json");
    expect(csv.headers.get("content-type")).toBe("text/csv; charset=utf-8");
    expect(csv.headers.get("content-disposition")).toBe('attachment; filename="contacts.csv"');
    expect(await csv.text()).toBe("id,first_name\r\n1,Elena\r\n");
  });

  it("on a 401 refreshes once, retries with the new access token and updates both cookies", async () => {
    const navis = fakeNavis({
      "GET /api/auth/me": (call) =>
        call.headers.get("authorization") === "Bearer AT2"
          ? Response.json(user)
          : Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 }),
      "POST /api/auth/refresh": () => Response.json(tokens(2)),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch, secureCookies: true });

    const response = await bff.proxy(
      request("/api/auth/me", { cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" } }),
      "/api/auth/me",
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual(user);
    expect(navis.calls.map((c) => `${c.method} ${new URL(c.url).pathname}`)).toEqual([
      "GET /api/auth/me",
      "POST /api/auth/refresh",
      "GET /api/auth/me",
    ]);
    expect(JSON.parse(navis.calls[1].body)).toEqual({ refresh_token: "RT1" });
    const cookies = setCookies(response);
    expect(cookies[ACCESS_COOKIE].value).toBe("AT2");
    expect(cookies[REFRESH_COOKIE].value).toBe("RT2");
    expect(cookies[ACCESS_COOKIE].attrs).toEqual(
      expect.arrayContaining(["Path=/", "HttpOnly", "SameSite=Lax", "Max-Age=900", "Secure"]),
    );
  });

  it("replays the request body after refreshing", async () => {
    let attempts = 0;
    const navis = fakeNavis({
      "POST /api/v1/deals/d1/move": (call) => {
        attempts += 1;
        return attempts === 1
          ? Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 })
          : Response.json({ moved: JSON.parse(call.body), auth: call.headers.get("authorization") });
      },
      "POST /api/auth/refresh": () => Response.json(tokens(2)),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.proxy(
      request("/api/v1/deals/d1/move", {
        method: "POST",
        cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" },
        body: { stage: "won" },
      }),
      "/api/v1/deals/d1/move",
    );

    expect(await response.json()).toEqual({ moved: { stage: "won" }, auth: "Bearer AT2" });
  });

  it("clears both cookies and returns 401 when the refresh fails", async () => {
    const navis = fakeNavis({
      "GET /api/v1/dashboard": () =>
        Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 }),
      "POST /api/auth/refresh": () =>
        Response.json({ statusCode: 401, message: "Refresh token reused", error: "Unauthorized" }, { status: 401 }),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.proxy(
      request("/api/v1/dashboard", { cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" } }),
      "/api/v1/dashboard",
    );

    expect(response.status).toBe(401);
    expect(await response.json()).toMatchObject({ statusCode: 401, error: "Unauthorized" });
    const cookies = setCookies(response);
    expect(cookies[ACCESS_COOKIE]).toMatchObject({ value: "" });
    expect(cookies[ACCESS_COOKIE].attrs).toContain("Max-Age=0");
    expect(cookies[REFRESH_COOKIE].attrs).toContain("Max-Age=0");
    expect(navis.calls).toHaveLength(2); // no retry after a failed refresh
  });

  it("refreshes up front when only the refresh cookie is left", async () => {
    const navis = fakeNavis({
      "GET /api/auth/me": (call) => Response.json({ auth: call.headers.get("authorization") }),
      "POST /api/auth/refresh": () => Response.json(tokens(5)),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.proxy(request("/api/auth/me", { cookies: { [REFRESH_COOKIE]: "RT4" } }), "/api/auth/me");

    expect(await response.json()).toEqual({ auth: "Bearer AT5" });
    expect(setCookies(response)[REFRESH_COOKIE].value).toBe("RT5");
  });

  it("shares one refresh between concurrent requests (refresh tokens rotate; reuse revokes the family)", async () => {
    let refreshCalls = 0;
    const navis = fakeNavis({
      "GET /api/v1/deals/board": (call) =>
        call.headers.get("authorization") === "Bearer AT2"
          ? Response.json({ ok: true })
          : Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 }),
      "POST /api/auth/refresh": async () => {
        refreshCalls += 1;
        await new Promise((r) => setTimeout(r, 10));
        return Response.json(tokens(2));
      },
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });
    const cookies = { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" };

    const responses = await Promise.all(
      [1, 2, 3].map(() => bff.proxy(request("/api/v1/deals/board", { cookies }), "/api/v1/deals/board")),
    );

    expect(responses.map((r) => r.status)).toEqual([200, 200, 200]);
    expect(refreshCalls).toBe(1);
  });

  it("does not hang on tee'd response bodies, as Next.js's server fetch returns them", async () => {
    const navis = fakeNavis({
      "GET /api/auth/me": (call) =>
        call.headers.get("authorization") === "Bearer AT2"
          ? Response.json(user)
          : Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 }),
      "POST /api/auth/refresh": () => Response.json(tokens(2)),
    });
    const kept: ReadableStream[] = [];
    const teeing = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const response = await navis.fetch(input, init);
      if (!response.body) return response;
      const [mine, theirs] = response.body.tee();
      kept.push(theirs); // Next keeps the other branch, so cancel() on ours would never settle.
      return new Response(mine, { status: response.status, headers: response.headers });
    }) as typeof fetch;
    const bff = createBff({ apiUrl: API, fetch: teeing });

    const response = await Promise.race([
      bff.proxy(
        request("/api/auth/me", { cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" } }),
        "/api/auth/me",
      ),
      new Promise<"hung">((resolve) => setTimeout(() => resolve("hung"), 1000)),
    ]);

    expect(response).not.toBe("hung");
    expect((response as Response).status).toBe(200);
    expect(kept.length).toBeGreaterThan(0);
  });

  it("returns 502 in the NestJS format when Navis is unreachable", async () => {
    const bff = createBff({
      apiUrl: API,
      fetch: (async () => {
        throw new TypeError("fetch failed");
      }) as typeof fetch,
    });

    const response = await bff.proxy(
      request("/api/v1/dashboard", { cookies: { [ACCESS_COOKIE]: "AT1" } }),
      "/api/v1/dashboard",
    );

    expect(response.status).toBe(502);
    expect(await response.json()).toEqual({
      statusCode: 502,
      message: "The Navis API is unreachable",
      error: "Bad Gateway",
    });
  });
});

describe("BFF origin check (CSRF defence for the cookie hop)", () => {
  const navis = () =>
    fakeNavis({
      "POST /api/v1/companies": () => Response.json({}, { status: 201 }),
      "GET /api/v1/companies": () => Response.json({}),
    });

  it("rejects non-GET requests from another origin with 403 and never calls Navis", async () => {
    const k = navis();
    const bff = createBff({ apiUrl: API, fetch: k.fetch });

    const response = await bff.proxy(
      request("/api/v1/companies", {
        method: "POST",
        origin: "https://evil.example",
        body: {},
        cookies: { [ACCESS_COOKIE]: "AT1" },
      }),
      "/api/v1/companies",
    );

    expect(response.status).toBe(403);
    expect(await response.json()).toEqual({ statusCode: 403, message: "Invalid origin", error: "Forbidden" });
    expect(k.calls).toHaveLength(0);
  });

  it("rejects non-GET requests without an Origin header", async () => {
    const k = navis();
    const bff = createBff({ apiUrl: API, fetch: k.fetch });

    const response = await bff.proxy(
      request("/api/v1/companies", { method: "POST", origin: null, body: {} }),
      "/api/v1/companies",
    );

    expect(response.status).toBe(403);
  });

  it("allows same-origin writes, cross-origin reads, and configured public origins", async () => {
    const k = navis();
    const bff = createBff({ apiUrl: API, fetch: k.fetch, allowedOrigins: ["https://crm.example.com"] });

    const sameOrigin = await bff.proxy(request("/api/v1/companies", { method: "POST", body: {} }), "/api/v1/companies");
    const read = await bff.proxy(request("/api/v1/companies", { origin: "https://evil.example" }), "/api/v1/companies");
    const proxied = await bff.proxy(
      request("/api/v1/companies", { method: "POST", origin: "https://crm.example.com", body: {} }),
      "/api/v1/companies",
    );
    const forwardedHost = await bff.proxy(
      request("/api/v1/companies", {
        method: "POST",
        origin: "https://crm.internal",
        headers: { "x-forwarded-host": "crm.internal" },
        body: {},
      }),
      "/api/v1/companies",
    );

    expect([sameOrigin.status, read.status, proxied.status, forwardedHost.status]).toEqual([201, 200, 201, 201]);
  });

  it("also guards login and logout", async () => {
    const k = navis();
    const bff = createBff({ apiUrl: API, fetch: k.fetch });

    const login = await bff.login(
      request("/api/auth/login", { method: "POST", origin: "https://evil.example", body: {} }),
    );
    const logout = await bff.logout(request("/api/auth/logout", { method: "POST", origin: "https://evil.example" }));

    expect([login.status, logout.status]).toEqual([403, 403]);
    expect(k.calls).toHaveLength(0);
  });
});

describe("BFF login and logout", () => {
  it("login stores both tokens in httpOnly cookies and returns only {user}", async () => {
    const navis = fakeNavis({ "POST /api/auth/login": () => Response.json({ user, ...tokens(1) }) });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch, secureCookies: false });

    const response = await bff.login(
      request("/api/auth/login", { method: "POST", body: { email: "rep1@navis.test", password: "password123" } }),
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ user });
    expect(JSON.parse(navis.calls[0].body)).toEqual({ email: "rep1@navis.test", password: "password123" });
    expect(navis.calls[0].headers.get("authorization")).toBeNull();
    const cookies = setCookies(response);
    expect(cookies[ACCESS_COOKIE].value).toBe("AT1");
    expect(cookies[REFRESH_COOKIE].value).toBe("RT1");
    for (const name of [ACCESS_COOKIE, REFRESH_COOKIE]) {
      expect(cookies[name].attrs).toEqual(expect.arrayContaining(["Path=/", "HttpOnly", "SameSite=Lax"]));
      expect(cookies[name].attrs).not.toContain("Secure");
    }
    expect(cookies[ACCESS_COOKIE].attrs).toContain("Max-Age=900");
    expect(cookies[REFRESH_COOKIE].attrs).toContain(`Max-Age=${60 * 60 * 24 * 30}`);
  });

  it("login passes Navis's error through and sets no cookies", async () => {
    const navis = fakeNavis({
      "POST /api/auth/login": () =>
        Response.json({ statusCode: 401, message: "Invalid credentials", error: "Unauthorized" }, { status: 401 }),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.login(
      request("/api/auth/login", { method: "POST", body: { email: "x@y.z", password: "no" } }),
    );

    expect(response.status).toBe(401);
    expect(await response.json()).toEqual({ statusCode: 401, message: "Invalid credentials", error: "Unauthorized" });
    expect(response.headers.getSetCookie()).toEqual([]);
  });

  it("logout revokes the refresh token at Navis with the bearer token and clears both cookies", async () => {
    const navis = fakeNavis({ "POST /api/auth/logout": () => new Response(null, { status: 204 }) });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.logout(
      request("/api/auth/logout", {
        method: "POST",
        cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" },
        body: { all: true },
      }),
    );

    expect(response.status).toBe(204);
    expect(navis.calls[0].headers.get("authorization")).toBe("Bearer AT1");
    expect(JSON.parse(navis.calls[0].body)).toEqual({ refresh_token: "RT1", all: true });
    const cookies = setCookies(response);
    expect(cookies[ACCESS_COOKIE].attrs).toContain("Max-Age=0");
    expect(cookies[REFRESH_COOKIE].attrs).toContain("Max-Age=0");
  });

  it("logout still clears the cookies when Navis is down", async () => {
    const bff = createBff({
      apiUrl: API,
      fetch: (async () => {
        throw new TypeError("fetch failed");
      }) as typeof fetch,
    });

    const response = await bff.logout(
      request("/api/auth/logout", { method: "POST", cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" } }),
    );

    expect(response.status).toBe(204);
    expect(setCookies(response)[REFRESH_COOKIE].attrs).toContain("Max-Age=0");
  });
});

describe("BFF two-factor sign-in", () => {
  const challenge = { two_factor_required: true, challenge_token: "CHALLENGE-JWT", expires_in: 300 };

  it("login keeps the challenge token in an httpOnly cookie and returns only {two_factor_required, expires_in}", async () => {
    const navis = fakeNavis({ "POST /api/auth/login": () => Response.json(challenge) });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch, secureCookies: true });

    const response = await bff.login(
      request("/api/auth/login", { method: "POST", body: { email: "boss@navis.test", password: "password123" } }),
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ two_factor_required: true, expires_in: 300 });
    const cookies = setCookies(response);
    expect(Object.keys(cookies)).toEqual([CHALLENGE_COOKIE]);
    expect(cookies[CHALLENGE_COOKIE].value).toBe("CHALLENGE-JWT");
    expect(cookies[CHALLENGE_COOKIE].attrs).toEqual(
      expect.arrayContaining(["Path=/api/auth/2fa", "HttpOnly", "SameSite=Lax", "Max-Age=300", "Secure"]),
    );
  });

  it("verify sends the challenge token from the cookie with the code, stores the tokens and returns only {user}", async () => {
    const navis = fakeNavis({ "POST /api/auth/2fa/verify": () => Response.json({ user, ...tokens(7) }) });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch, secureCookies: false });

    const response = await bff.verifyTwoFactor(
      request("/api/auth/2fa/verify", {
        method: "POST",
        cookies: { [CHALLENGE_COOKIE]: "CHALLENGE-JWT" },
        body: { code: "123456", challenge_token: "ignored-from-the-browser" },
      }),
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ user });
    expect(JSON.parse(navis.calls[0].body)).toEqual({ challenge_token: "CHALLENGE-JWT", code: "123456" });
    expect(navis.calls[0].headers.get("authorization")).toBeNull();
    const cookies = setCookies(response);
    expect(cookies[ACCESS_COOKIE].value).toBe("AT7");
    expect(cookies[REFRESH_COOKIE].value).toBe("RT7");
    expect(cookies[ACCESS_COOKIE].attrs).toEqual(expect.arrayContaining(["Path=/", "HttpOnly"]));
    expect(cookies[CHALLENGE_COOKIE].attrs).toEqual(expect.arrayContaining(["Path=/api/auth/2fa", "Max-Age=0"]));
  });

  it("verify forwards a recovery code instead of a code", async () => {
    const navis = fakeNavis({ "POST /api/auth/2fa/verify": () => Response.json({ user, ...tokens(8) }) });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.verifyTwoFactor(
      request("/api/auth/2fa/verify", {
        method: "POST",
        cookies: { [CHALLENGE_COOKIE]: "CHALLENGE-JWT" },
        body: { recovery_code: "abcd-efgh" },
      }),
    );

    expect(response.status).toBe(200);
    expect(JSON.parse(navis.calls[0].body)).toEqual({ challenge_token: "CHALLENGE-JWT", recovery_code: "abcd-efgh" });
  });

  it("verify passes a wrong code's error through and keeps the challenge cookie for another try", async () => {
    const navis = fakeNavis({
      "POST /api/auth/2fa/verify": () =>
        Response.json({ statusCode: 401, message: "Invalid two-factor code", error: "Unauthorized" }, { status: 401 }),
    });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.verifyTwoFactor(
      request("/api/auth/2fa/verify", {
        method: "POST",
        cookies: { [CHALLENGE_COOKIE]: "CHALLENGE-JWT" },
        body: { code: "000000" },
      }),
    );

    expect(response.status).toBe(401);
    expect(await response.json()).toEqual({
      statusCode: 401,
      message: "Invalid two-factor code",
      error: "Unauthorized",
    });
    expect(response.headers.getSetCookie()).toEqual([]);
  });

  it("verify without a challenge cookie is a 401 that never reaches Navis", async () => {
    const navis = fakeNavis({});
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.verifyTwoFactor(
      request("/api/auth/2fa/verify", { method: "POST", body: { code: "123456" } }),
    );

    expect(response.status).toBe(401);
    expect((await response.json()).message).toBe("Your sign-in attempt expired. Sign in again.");
    expect(navis.calls).toHaveLength(0);
  });

  it("verify is guarded by the origin check", async () => {
    const navis = fakeNavis({});
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.verifyTwoFactor(
      request("/api/auth/2fa/verify", {
        method: "POST",
        origin: "http://evil.test",
        cookies: { [CHALLENGE_COOKIE]: "CHALLENGE-JWT" },
        body: { code: "123456" },
      }),
    );

    expect(response.status).toBe(403);
    expect(navis.calls).toHaveLength(0);
  });
});

describe("BFF keeps tokens out of proxied responses", () => {
  it("a token pair returned by Navis (PATCH /api/auth/me/password) replaces the cookies and is not sent to the browser", async () => {
    const navis = fakeNavis({ "PATCH /api/auth/me/password": () => Response.json(tokens(9)) });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.proxy(
      request("/api/auth/me/password", {
        method: "PATCH",
        cookies: { [ACCESS_COOKIE]: "AT1", [REFRESH_COOKIE]: "RT1" },
        body: { current_password: "password123", password: "password456" },
      }),
      "/api/auth/me/password",
    );

    expect(response.status).toBe(204);
    expect(await response.text()).toBe("");
    const cookies = setCookies(response);
    expect(cookies[ACCESS_COOKIE].value).toBe("AT9");
    expect(cookies[REFRESH_COOKIE].value).toBe("RT9");
  });

  it("the login body of a proxied 2FA verify is reduced to {user}", async () => {
    const navis = fakeNavis({ "POST /api/auth/2fa/verify": () => Response.json({ user, ...tokens(10) }) });
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    const response = await bff.proxy(
      request("/api/auth/2fa/verify", { method: "POST", body: { challenge_token: "X", code: "123456" } }),
      "/api/auth/2fa/verify",
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ user });
    expect(setCookies(response)[ACCESS_COOKIE].value).toBe("AT10");
  });
});

describe("BFF client address for Navis's per-IP limits (X-Forwarded-For)", () => {
  // What a browser can send. Next.js keeps a client's X-Forwarded-For as is (it only fills the header in when
  // it is missing), so these values reach the route handlers untouched.
  const spoofed = { "x-forwarded-for": "6.6.6.6", "x-real-ip": "6.6.6.6" };
  const everyRoute = () =>
    fakeNavis({
      "GET /api/v1/me": () => Response.json({ id: "u1" }),
      "POST /api/auth/login": () => Response.json({ user, ...tokens(1) }),
      "POST /api/auth/refresh": () => Response.json(tokens(2)),
      "POST /api/auth/2fa/verify": () => Response.json({ user, ...tokens(3) }),
      "POST /api/auth/logout": () => new Response(null, { status: 204 }),
    });

  /** Every upstream call the BFF makes for one browser request of each kind. */
  async function callEverything(bff: ReturnType<typeof createBff>, headers: Record<string, string>) {
    await bff.proxy(request("/api/v1/me", { headers, cookies: { [REFRESH_COOKIE]: "RT1" } }), "/api/v1/me");
    await bff.login(
      request("/api/auth/login", { method: "POST", headers, body: { email: "rep1@navis.test", password: "x" } }),
    );
    await bff.verifyTwoFactor(
      request("/api/auth/2fa/verify", {
        method: "POST",
        headers,
        cookies: { [CHALLENGE_COOKIE]: "CHALLENGE" },
        body: { code: "123456" },
      }),
    );
    await bff.logout(request("/api/auth/logout", { method: "POST", headers, cookies: { [REFRESH_COOKIE]: "RT5" } }));
  }

  it("never passes a browser's X-Forwarded-For or X-Real-IP on to Navis when no trusted proxy is configured", async () => {
    const navis = everyRoute();
    const bff = createBff({ apiUrl: API, fetch: navis.fetch });

    await callEverything(bff, spoofed);

    expect(navis.calls.map((c) => `${c.method} ${new URL(c.url).pathname}`)).toEqual([
      "POST /api/auth/refresh",
      "GET /api/v1/me",
      "POST /api/auth/login",
      "POST /api/auth/2fa/verify",
      "POST /api/auth/refresh",
      "POST /api/auth/logout",
    ]);
    for (const call of navis.calls) {
      expect(call.headers.get("x-forwarded-for")).toBeNull();
      expect(call.headers.get("x-real-ip")).toBeNull();
    }
  });

  it("behind trusted proxies, sends Navis only the address the outermost trusted proxy saw", async () => {
    // One proxy in front of Next.js: it appended the real client (203.0.113.7) to what the browser sent.
    const one = everyRoute();
    await callEverything(createBff({ apiUrl: API, fetch: one.fetch, trustedProxyHops: 1 }), {
      "x-forwarded-for": "6.6.6.6, 203.0.113.7",
      "x-real-ip": "6.6.6.6",
    });
    expect(one.calls).toHaveLength(6);
    for (const call of one.calls) {
      expect(call.headers.get("x-forwarded-for")).toBe("203.0.113.7");
      expect(call.headers.get("x-real-ip")).toBeNull();
    }

    // A CDN and a load balancer: the CDN saw the client, the load balancer appended the CDN's address.
    const two = fakeNavis({ "GET /api/v1/me": () => Response.json({ id: "u1" }) });
    await createBff({ apiUrl: API, fetch: two.fetch, trustedProxyHops: 2 }).proxy(
      request("/api/v1/me", { headers: { "x-forwarded-for": "6.6.6.6,2001:db8::7 , 10.0.0.2" } }),
      "/api/v1/me",
    );
    expect(two.calls[0].headers.get("x-forwarded-for")).toBe("2001:db8::7");
  });

  it("sends no address when the trusted proxies' entry is missing or not an IP address", async () => {
    const cases: [number, string | undefined][] = [
      [1, undefined],
      [1, "not-an-ip"],
      [1, "6.6.6.6, "],
      [2, "203.0.113.7"],
    ];
    for (const [trustedProxyHops, forwardedFor] of cases) {
      const navis = fakeNavis({ "GET /api/v1/me": () => Response.json({ id: "u1" }) });
      const headers: Record<string, string> = forwardedFor === undefined ? {} : { "x-forwarded-for": forwardedFor };
      await createBff({ apiUrl: API, fetch: navis.fetch, trustedProxyHops }).proxy(
        request("/api/v1/me", { headers }),
        "/api/v1/me",
      );
      expect(navis.calls[0].headers.get("x-forwarded-for"), `${trustedProxyHops} hop(s), ${forwardedFor}`).toBeNull();
    }
  });
});
