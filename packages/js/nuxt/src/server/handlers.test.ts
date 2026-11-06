// @vitest-environment node
import { ACCESS_COOKIE, createBff, REFRESH_COOKIE, WORKSPACE_COOKIE } from "@navree/bff";
import { browserRequest, fakeNavree, readSetCookies, tokenPair, unsignedJwt } from "@navree/bff/testing";
import { createApp, createRouter, defineEventHandler, getRequestHeader, toWebHandler } from "h3";
import { describe, expect, it } from "vitest";
import { bffResolver, createBffHandler, createSessionGate, getNavreeSession } from "./handlers.js";

const API = "http://navree.test";
const APP = "http://app.test";
const user = { __type: "User", id: "u1", name: "Anu" };

function navreeApi() {
  return fakeNavree({
    "POST /api/auth/login": () => Response.json({ user, ...tokenPair(1) }),
    "POST /api/auth/refresh": () => Response.json(tokenPair(2)),
    "GET /api/auth/me": (call) =>
      call.headers.get("authorization") === "Bearer AT1"
        ? Response.json(user)
        : Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 }),
    "GET /api/v1/deals": (call) => Response.json({ auth: call.headers.get("authorization") }),
    "POST /broadcasting/auth": () => Response.json({ auth: "key:sig" }),
    "GET /api/operations/op1/events": () =>
      new Response("event: operation\ndata: {}\n\n", { headers: { "content-type": "text/event-stream" } }),
  });
}

/** A Nitro-like app: the BFF on /api/** and /broadcasting/auth, an echo page for the gate. */
function server(navree = navreeApi(), gate?: Parameters<typeof createSessionGate>[1]) {
  const bff = createBff({ apiUrl: API, fetch: navree.fetch });
  const app = createApp();
  if (gate) app.use(createSessionGate(() => bff, gate));
  const router = createRouter();
  const handler = createBffHandler(() => bff);
  router.use("/api/**", handler);
  router.post("/broadcasting/auth", handler);
  router.get(
    "/**",
    defineEventHandler((event) => ({ page: event.path, cookie: getRequestHeader(event, "cookie") ?? null })),
  );
  app.use(router);
  return { web: toWebHandler(app), navree, bff };
}

describe("the BFF as Nitro routes", () => {
  it("signs in with httpOnly cookies and returns only the user", async () => {
    const { web } = server();
    const response = await web(browserRequest(APP, "/api/auth/login", { method: "POST", body: { email: "a", password: "b" } }));
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ user });
    const cookies = readSetCookies(response);
    expect(cookies[ACCESS_COOKIE]!.value).toBe("AT1");
    expect(cookies[REFRESH_COOKIE]!.attrs).toContain("HttpOnly");
  });

  it("proxies API calls and channel authorization with the bearer token", async () => {
    const { web } = server();
    const deals = await web(browserRequest(APP, "/api/v1/deals", { cookies: { [ACCESS_COOKIE]: "AT1" } }));
    expect(await deals.json()).toEqual({ auth: "Bearer AT1" });

    const channel = await web(
      browserRequest(APP, "/broadcasting/auth", { method: "POST", cookies: { [ACCESS_COOKIE]: "AT1" }, body: { socket_id: "1.2", channel_name: "private-x" } }),
    );
    expect(await channel.json()).toEqual({ auth: "key:sig" });
  });

  it("refuses a write from another origin", async () => {
    const { web, navree } = server();
    const response = await web(browserRequest(APP, "/api/v1/deals", { method: "POST", origin: "https://evil.example", body: {} }));
    expect(response.status).toBe(403);
    expect((await response.json()).code).toBe("invalid_origin");
    expect(navree.calls).toHaveLength(0);
  });

  it("streams an operation's events", async () => {
    const { web } = server();
    const response = await web(browserRequest(APP, "/api/operations/op1/events", { cookies: { [ACCESS_COOKIE]: "AT1" } }));
    expect(response.headers.get("content-type")).toBe("text/event-stream");
    expect(await response.text()).toBe("event: operation\ndata: {}\n\n");
  });
});

describe("bffResolver", () => {
  it("makes one BFF per API URL, so every request shares its refresh map", () => {
    const resolve = bffResolver(() => ({ apiUrl: API }));
    expect(resolve()).toBe(resolve());
    let url = API;
    const moving = bffResolver(() => ({ apiUrl: url }));
    const first = moving();
    url = "http://other.test";
    expect(moving()).not.toBe(first);
  });
});

describe("the session gate", () => {
  const page = (path: string, cookies: Record<string, string> = {}, accept = "text/html") =>
    browserRequest(APP, path, { cookies, headers: { accept } });

  it("sends a browser without a session to sign-in", async () => {
    const { web } = server(undefined, { loginPath: "/login" });
    const response = await web(page("/deals?stage=won"));
    expect(response.status).toBe(302);
    expect(response.headers.get("location")).toBe("/login?next=%2Fdeals%3Fstage%3Dwon");
  });

  it("lets public pages, the BFF, assets and API calls through", async () => {
    const { web } = server(undefined, { loginPath: "/login", publicPaths: ["/", "/docs/*"] });
    for (const path of ["/login", "/", "/docs/start", "/_nuxt/app.js", "/favicon.ico"]) {
      const response = await web(page(path));
      expect(response.status, path).toBe(200);
    }
    const api = await web(page("/api/v1/deals", {}, "application/json"));
    expect(api.status).toBe(200);
    const json = await web(page("/deals", {}, "application/json"));
    expect(json.status).toBe(200);
  });

  it("refreshes an expired token before the page renders, for the browser and for this request", async () => {
    const { web } = server(undefined, {});
    const stale = unsignedJwt({ sub: "u1", exp: Math.floor(Date.now() / 1000) - 60 });

    const response = await web(page("/deals", { [ACCESS_COOKIE]: stale, [REFRESH_COOKIE]: "RT1" }));

    expect(response.status).toBe(200);
    expect(readSetCookies(response)[ACCESS_COOKIE]!.value).toBe("AT2");
    const body = (await response.json()) as { cookie: string };
    expect(body.cookie).toContain(`${ACCESS_COOKIE}=AT2`);
  });

  it("clears an expired session and sends the browser to sign-in", async () => {
    const navree = fakeNavree({
      "POST /api/auth/refresh": () => Response.json({ statusCode: 401, message: "x", error: "Unauthorized" }, { status: 401 }),
    });
    const { web } = server(navree, {});
    const response = await web(page("/deals", { [REFRESH_COOKIE]: "RT1" }));
    expect(response.status).toBe(302);
    expect(readSetCookies(response)[REFRESH_COOKIE]!.attrs).toContain("Max-Age=0");
  });
});

describe("the session gate when the API is down", () => {
  const page = (path: string, cookies: Record<string, string> = {}, accept = "text/html") =>
    browserRequest(APP, path, { cookies, headers: { accept } });

  it("serves the page and keeps the cookies when the token can't be refreshed because the API can't be reached", async () => {
    const navree = fakeNavree({ "POST /api/auth/refresh": () => new Response(null, { status: 503 }) });
    const { web } = server(navree, {});
    const stale = unsignedJwt({ sub: "u1", exp: Math.floor(Date.now() / 1000) - 60 });

    const response = await web(page("/deals", { [ACCESS_COOKIE]: stale, [REFRESH_COOKIE]: "RT1" }));

    expect(response.status).toBe(200);
    expect(response.headers.getSetCookie()).toEqual([]);
    expect(((await response.json()) as { page: string }).page).toBe("/deals");
  });

  it("lets the social sign-in routes through for a browser without a session", async () => {
    const { web } = server(undefined, { loginPath: "/login" });
    for (const path of ["/auth/social/github", "/auth/callback/github?code=c&state=s"]) {
      const response = await web(page(path));
      expect(response.status, path).toBe(200);
    }
  });
});

describe("social sign-in and the verification link as Nitro routes", () => {
  it("are answered by the same handler as the rest of the BFF", async () => {
    const navree = fakeNavree({
      "GET /api/auth/social/github/redirect": () => Response.json({ url: "https://github.com/login/oauth/authorize?state=S1", state: "S1" }),
      "GET /api/auth/email/verify/u1/h1": () => new Response(null, { status: 204 }),
    });
    const bff = createBff({ apiUrl: API, fetch: navree.fetch });
    const app = createApp();
    const router = createRouter();
    const handler = createBffHandler(() => bff);
    router.get("/auth/social/**", handler);
    router.get("/api/**", handler);
    app.use(router);
    const web = toWebHandler(app);

    const start = await web(browserRequest(APP, "/auth/social/github?next=/"));
    expect(start.status).toBe(303);
    expect(start.headers.get("location")).toBe("https://github.com/login/oauth/authorize?state=S1");
    expect(readSetCookies(start).navree_social!.value).toBe("S1");

    const verified = await web(browserRequest(APP, "/api/auth/email/verify/u1/h1?signature=s"));
    expect(verified.headers.get("location")).toBe("/verify-email?status=verified");
  });
});

describe("getNavreeSession (server routes and server-side rendering)", () => {
  it("returns the user, token and workspace of a request, or null", async () => {
    const navree = navreeApi();
    const bff = createBff({ apiUrl: API, fetch: navree.fetch });
    const app = createApp();
    app.use(defineEventHandler(async (event) => ({ session: await getNavreeSession(event, bff) })));
    const web = toWebHandler(app);

    const signedIn = await web(page2({ [ACCESS_COOKIE]: "AT1", [WORKSPACE_COOKIE]: "agra" }));
    expect(await signedIn.json()).toEqual({ session: { user, accessToken: "AT1", workspace: "agra" } });
    const anonymous = await web(page2({}));
    expect(await anonymous.json()).toEqual({ session: null });
  });
});

function page2(cookies: Record<string, string>) {
  return browserRequest(APP, "/me", { cookies });
}
