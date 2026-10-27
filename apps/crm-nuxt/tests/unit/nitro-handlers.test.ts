// @vitest-environment node
/**
 * The BFF as Nitro serves it: the real route handlers and the session-gate middleware from
 * `server/`, mounted on an h3 app the way Nitro's file routing mounts them, called with web
 * Requests. Nitro's auto-imports are the h3 functions of the same name; `useRuntimeConfig`
 * is stubbed. Navree is a scripted fake behind `fetch`.
 */
import * as h3 from "h3";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

const API = "http://navree.test";
const APP = "http://crm.test";

const runtimeConfig = { navreeApiUrl: "", public: { apiMocks: false } };
for (const name of [
  "defineEventHandler",
  "toWebRequest",
  "getRequestURL",
  "getRequestHeader",
  "getCookie",
  "sendRedirect",
] as const) {
  vi.stubGlobal(name, h3[name]);
}
vi.stubGlobal("useRuntimeConfig", () => runtimeConfig);
process.env.NAVREE_API_URL = API;

type Call = { url: URL; method: string; headers: Headers; body: string };
const calls: Call[] = [];
let routes: Record<string, (call: Call) => Response> = {};

const navree = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
  const raw = init?.body;
  const body =
    raw === undefined || raw === null ? "" : typeof raw === "string" ? raw : new TextDecoder().decode(raw as ArrayBuffer);
  const call: Call = { url: new URL(String(input)), method: (init?.method ?? "GET").toUpperCase(), headers: new Headers(init?.headers), body };
  calls.push(call);
  const route = routes[`${call.method} ${call.url.pathname}`];
  return route ? route(call) : Response.json({ statusCode: 404, message: "Not Found", error: "Not Found" }, { status: 404 });
});

let handle: (request: Request) => Promise<Response>;

beforeAll(async () => {
  vi.stubGlobal("fetch", navree);
  const load = async (path: string) => ((await import(path)) as { default: h3.EventHandler }).default;
  const app = h3.createApp();
  app.use(await load("../../server/middleware/session-gate"));
  const router = h3.createRouter();
  router.post("/api/auth/login", await load("../../server/api/auth/login.post"));
  router.post("/api/auth/logout", await load("../../server/api/auth/logout.post"));
  router.post("/api/auth/refresh", await load("../../server/api/auth/refresh.post"));
  router.post("/api/auth/2fa/verify", await load("../../server/api/auth/2fa/verify.post"));
  router.use("/api/**", await load("../../server/api/[...path]"));
  router.post("/broadcasting/auth", await load("../../server/routes/broadcasting/auth.post"));
  // Everything else is the single-page app.
  router.get("/**", h3.defineEventHandler(() => "<!doctype html><div id=__nuxt></div>"));
  app.use(router);
  handle = h3.toWebHandler(app);
});

afterAll(() => {
  vi.unstubAllGlobals();
  delete process.env.NAVREE_API_URL;
});

beforeEach(() => {
  calls.length = 0;
  routes = {};
  runtimeConfig.public.apiMocks = false;
});

function request(
  path: string,
  { method = "GET", cookies = {}, origin, body, accept }: { method?: string; cookies?: Record<string, string>; origin?: string; body?: unknown; accept?: string } = {},
): Request {
  const headers = new Headers({ host: new URL(APP).host });
  const cookie = Object.entries(cookies)
    .map(([k, v]) => `${k}=${v}`)
    .join("; ");
  if (cookie) headers.set("cookie", cookie);
  if (origin) headers.set("origin", origin);
  else if (method !== "GET") headers.set("origin", APP);
  if (accept) headers.set("accept", accept);
  if (body !== undefined) headers.set("content-type", "application/json");
  return new Request(`${APP}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
}

function setCookies(response: Response): Record<string, string> {
  return Object.fromEntries(
    response.headers.getSetCookie().map((line) => {
      const pair = line.split("; ")[0]!;
      const eq = pair.indexOf("=");
      return [pair.slice(0, eq), decodeURIComponent(pair.slice(eq + 1))];
    }),
  );
}

const html = "text/html,application/xhtml+xml";
const user = { __type: "User", id: "u1", name: "Priya Shah", email: "rep1@navree.test", roles: ["rep"] };

describe("session gate (server/middleware/session-gate.ts)", () => {
  it("sends a browser with no session cookie to /login, keeping where it was going", async () => {
    const home = await handle(request("/", { accept: html }));
    expect(home.status).toBe(302);
    expect(home.headers.get("location")).toBe("/login");

    const deal = await handle(request("/deals/d1?tab=activity", { accept: html }));
    expect(deal.status).toBe(302);
    expect(deal.headers.get("location")).toBe(`/login?next=${encodeURIComponent("/deals/d1?tab=activity")}`);
  });

  it("lets the app load with either token cookie; whether it is still valid is the API's call", async () => {
    expect((await handle(request("/deals", { accept: html, cookies: { navree_rt: "RT1" } }))).status).toBe(200);
    expect((await handle(request("/deals", { accept: html, cookies: { navree_at: "AT1" } }))).status).toBe(200);
  });

  it("never gates the login page, assets, the BFF or non-HTML requests", async () => {
    expect((await handle(request("/login", { accept: html }))).status).toBe(200);
    expect((await handle(request("/_nuxt/entry.js", { accept: "*/*" }))).status).toBe(200);
    expect((await handle(request("/icon.svg", { accept: html }))).status).toBe(200);
    routes["GET /api/auth/me"] = () => Response.json({ statusCode: 401, message: "Unauthenticated", error: "Unauthorized" }, { status: 401 });
    expect((await handle(request("/api/auth/me", { accept: html }))).status).toBe(401);
  });

  it("is off with the mock API, whose session lives in the tab", async () => {
    runtimeConfig.public.apiMocks = true;
    expect((await handle(request("/deals", { accept: html }))).status).toBe(200);
  });
});

describe("BFF routes (server/api, server/routes)", () => {
  it("POST /api/auth/login keeps both tokens in httpOnly cookies and answers only {user}", async () => {
    routes["POST /api/auth/login"] = () =>
      Response.json({ access_token: "AT1", refresh_token: "RT1", token_type: "Bearer", expires_in: 900, user });

    const response = await handle(request("/api/auth/login", { method: "POST", body: { email: user.email, password: "password123" } }));

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ user });
    expect(setCookies(response)).toEqual({ navree_at: "AT1", navree_rt: "RT1" });
    for (const line of response.headers.getSetCookie()) expect(line).toContain("HttpOnly");
    expect(JSON.parse(calls[0]!.body)).toEqual({ email: user.email, password: "password123" });
  });

  it("GET /api/* reaches Navree at the same path and query with the bearer token, never the cookies", async () => {
    routes["GET /api/v1/deals"] = () => Response.json({ data: [], meta: { total: 0 } });

    const response = await handle(request("/api/v1/deals?stage=lead&page=2", { cookies: { navree_at: "AT1", navree_rt: "RT1" } }));

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ data: [], meta: { total: 0 } });
    expect(calls[0]!.url.href).toBe(`${API}/api/v1/deals?stage=lead&page=2`);
    expect(calls[0]!.headers.get("authorization")).toBe("Bearer AT1");
    expect(calls[0]!.headers.get("cookie")).toBeNull();
  });

  it("refreshes an expired session once and rotates both cookies through Nitro", async () => {
    routes["GET /api/v1/dashboard"] = (call) =>
      call.headers.get("authorization") === "Bearer AT2"
        ? Response.json({ ok: true })
        : Response.json({ statusCode: 401, message: "Token expired", error: "Unauthorized" }, { status: 401 });
    routes["POST /api/auth/refresh"] = () => Response.json({ access_token: "AT2", refresh_token: "RT2", expires_in: 900 });

    const response = await handle(request("/api/v1/dashboard", { cookies: { navree_at: "AT1", navree_rt: "RT1" } }));

    expect(response.status).toBe(200);
    expect(setCookies(response)).toEqual({ navree_at: "AT2", navree_rt: "RT2" });
    expect(calls.map((c) => `${c.method} ${c.url.pathname}`)).toEqual([
      "GET /api/v1/dashboard",
      "POST /api/auth/refresh",
      "GET /api/v1/dashboard",
    ]);
  });

  it("refuses a mutation from another origin with 403 before it reaches Navree (CSRF)", async () => {
    const response = await handle(
      request("/api/v1/deals/d1/move", { method: "PATCH", origin: "https://evil.example", cookies: { navree_at: "AT1" }, body: { stage: "won" } }),
    );
    expect(response.status).toBe(403);
    expect(await response.json()).toEqual({ statusCode: 403, message: "Invalid origin", error: "Forbidden" });
    expect(calls).toHaveLength(0);
  });

  it("POST /broadcasting/auth authorizes a private channel at Navree with the bearer token", async () => {
    routes["POST /broadcasting/auth"] = () => Response.json({ auth: "navree-crm-key:sig" });

    const response = await handle(
      request("/broadcasting/auth", { method: "POST", cookies: { navree_at: "AT1" }, body: { socket_id: "1.2", channel_name: "private-crm.deals" } }),
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ auth: "navree-crm-key:sig" });
    expect(calls[0]!.url.href).toBe(`${API}/broadcasting/auth`);
    expect(calls[0]!.headers.get("authorization")).toBe("Bearer AT1");
    expect(JSON.parse(calls[0]!.body)).toEqual({ socket_id: "1.2", channel_name: "private-crm.deals" });
  });

  it("POST /api/auth/logout revokes the refresh token at Navree and clears both cookies", async () => {
    routes["POST /api/auth/logout"] = () => new Response(null, { status: 204 });

    const response = await handle(request("/api/auth/logout", { method: "POST", cookies: { navree_at: "AT1", navree_rt: "RT1" }, body: {} }));

    expect(response.status).toBe(204);
    expect(setCookies(response)).toEqual({ navree_at: "", navree_rt: "" });
    expect(calls[0]!.headers.get("authorization")).toBe("Bearer AT1");
    expect(JSON.parse(calls[0]!.body)).toEqual({ refresh_token: "RT1" });
  });
});
