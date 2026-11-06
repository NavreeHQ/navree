// @vitest-environment node
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import * as apiRoute from "@/app/api/[...path]/route";
import * as channelAuthRoute from "@/app/broadcasting/auth/route";

const APP = fileURLToPath(new URL("../../", import.meta.url));
const SRC = fileURLToPath(new URL("../", import.meta.url));
const ORIGIN = "http://crm.test";

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = `${dir}${name}`;
    if (statSync(path).isDirectory()) return sourceFiles(`${path}/`);
    return /\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}

describe("the app on the Navree packages", () => {
  it("depends on @navree/next and @navree/client, linked from the checkout's packages", () => {
    const manifest = JSON.parse(readFileSync(`${APP}package.json`, "utf8")) as { dependencies: Record<string, string> };
    expect(manifest.dependencies["@navree/next"]).toBe("link:../../packages/js/next");
    expect(manifest.dependencies["@navree/client"]).toBe("link:../../packages/js/client");
  });

  it("carries no backend-for-frontend of its own", () => {
    expect(existsSync(`${SRC}lib/bff`)).toBe(false);
    for (const file of sourceFiles(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const name of ["createBff", "ACCESS_COOKIE", "REFRESH_COOKIE", "navree_at", "navree_rt", "originAllowed"]) {
        expect(text.includes(name), `${file} mentions ${name}`).toBe(false);
      }
    }
  });

  it("mounts the package's handlers on the catch-all and on the channel authorization route", () => {
    for (const method of ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"] as const) {
      expect(typeof apiRoute[method], method).toBe("function");
    }
    expect(typeof channelAuthRoute.POST).toBe("function");
    expect(apiRoute.dynamic).toBe("force-dynamic");
  });
});

describe("the app's routes", () => {
  const user = { __type: "User", id: "u1", name: "Priya Verma", email: "priya@navree.test" };
  const upstream = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>();

  beforeAll(() => {
    // Settings are read when the first request arrives.
    process.env.NAVREE_API_URL = "http://api.test";
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    upstream.mockReset();
  });

  function browser(path: string, init: { method?: string; headers?: Record<string, string>; body?: string } = {}): Request {
    return new Request(`${ORIGIN}${path}`, { ...init, headers: { host: "crm.test", ...init.headers } });
  }

  function stubApi(handler: (url: URL, init?: RequestInit) => Response) {
    upstream.mockImplementation(async (url, init) => handler(new URL(url), init));
    vi.stubGlobal("fetch", upstream);
  }

  it("signs in through the catch-all: tokens go into httpOnly cookies, the page gets only the user", async () => {
    stubApi((url) =>
      url.pathname === "/api/auth/login"
        ? Response.json({ user, access_token: "AT1", refresh_token: "RT1", expires_in: 900 })
        : new Response(null, { status: 404 }),
    );

    const response = await apiRoute.POST(
      browser("/api/auth/login", {
        method: "POST",
        headers: { origin: ORIGIN, "content-type": "application/json" },
        body: JSON.stringify({ email: user.email, password: "password123" }),
      }),
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ user });
    const cookies = response.headers.getSetCookie();
    expect(cookies.find((c) => c.startsWith("navree_at=AT1"))).toMatch(/HttpOnly/);
    expect(cookies.find((c) => c.startsWith("navree_rt=RT1"))).toMatch(/HttpOnly/);
    expect(upstream.mock.calls[0]?.[0]).toBe("http://api.test/api/auth/login");
  });

  it("forwards other calls with the bearer token from the cookie", async () => {
    stubApi((url, init) =>
      Response.json({ path: url.pathname + url.search, authorization: new Headers(init?.headers).get("authorization") }),
    );

    const response = await apiRoute.GET(
      browser("/api/v1/deals?filter%5Bstage%5D=won", { headers: { cookie: "navree_at=AT1; navree_rt=RT1" } }),
    );

    expect(await response.json()).toEqual({ path: "/api/v1/deals?filter%5Bstage%5D=won", authorization: "Bearer AT1" });
  });

  it("refuses a write from another origin before it reaches the API", async () => {
    stubApi(() => Response.json({}));

    const response = await apiRoute.POST(
      browser("/api/v1/deals", {
        method: "POST",
        headers: { origin: "https://evil.example", cookie: "navree_at=AT1; navree_rt=RT1" },
      }),
    );

    expect(response.status).toBe(403);
    expect(await response.json()).toMatchObject({ code: "invalid_origin" });
    expect(upstream).not.toHaveBeenCalled();
  });

  it("authorizes a private channel through the BFF, with the bearer token", async () => {
    stubApi((url, init) => Response.json({ auth: "key:sig", at: url.pathname, authorization: new Headers(init?.headers).get("authorization") }));

    const response = await channelAuthRoute.POST(
      browser("/broadcasting/auth", {
        method: "POST",
        headers: { origin: ORIGIN, "content-type": "application/json", cookie: "navree_at=AT1; navree_rt=RT1" },
        body: JSON.stringify({ socket_id: "1.2", channel_name: "private-crm.deals" }),
      }),
    );

    expect(await response.json()).toMatchObject({ at: "/broadcasting/auth", authorization: "Bearer AT1" });
  });
});
