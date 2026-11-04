// @vitest-environment node
import { createBff } from "@navree/next/server";
import { describe, expect, it } from "vitest";
import { socialCallback, socialRedirect, verifyEmail } from "./social";

type Call = { method: string; url: URL; body: string };

/** A scripted Navree API for the BFF's `fetch`: routes by `"METHOD /path"`; a route may throw (unreachable). */
function scripted(routes: Record<string, (call: Call) => Response>) {
  const calls: Call[] = [];
  const fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const call: Call = {
      method: (init?.method ?? "GET").toUpperCase(),
      url: new URL(String(input)),
      body: typeof init?.body === "string" ? init.body : "",
    };
    calls.push(call);
    const route = routes[`${call.method} ${call.url.pathname}`];
    if (!route) return Response.json({ statusCode: 404, message: "Not Found", error: "Not Found" }, { status: 404 });
    return route(call);
  }) as typeof globalThis.fetch;
  return { bff: createBff({ apiUrl: "http://api.test", fetch, secureCookies: false }), calls };
}

const app = (path: string, cookie?: string) =>
  new Request(`http://app.test${path}`, { headers: cookie ? { cookie } : {} });

const cookiesOf = (response: Response) => response.headers.getSetCookie();
const location = (response: Response) => response.headers.get("location");

describe("social sign-in: the redirect to the provider", () => {
  it("sends the browser to the provider's URL and keeps the state in a cookie only the callback receives", async () => {
    const { bff, calls } = scripted({
      "GET /api/auth/social/github/redirect": () =>
        Response.json({ url: "https://github.example/authorize?client_id=x", state: "S1" }),
    });

    const response = await socialRedirect(bff, app("/auth/social/github?next=/w/acme"), "github");

    expect(response.status).toBe(303);
    expect(location(response)).toBe("https://github.example/authorize?client_id=x");
    expect(cookiesOf(response)).toEqual(["navree_social=S1; Path=/auth/callback; HttpOnly; SameSite=Lax; Max-Age=600"]);
    expect(calls[0].url.searchParams.get("return_to")).toBe("/w/acme");
  });

  it("never lets `next` leave this site", async () => {
    const { bff, calls } = scripted({
      "GET /api/auth/social/github/redirect": () => Response.json({ url: "https://github.example/a", state: "S1" }),
    });

    await socialRedirect(bff, app("/auth/social/github?next=//evil.example/x"), "github");

    expect(calls[0].url.searchParams.get("return_to")).toBe("/");
  });

  it("goes back to sign-in when the provider is not set up or the API is down", async () => {
    const missing = scripted({});
    const down = scripted({
      "GET /api/auth/social/github/redirect": () => {
        throw new TypeError("fetch failed");
      },
    });

    expect(location(await socialRedirect(missing.bff, app("/auth/social/github"), "github"))).toBe(
      "/sign-in?error=social_unavailable",
    );
    expect(location(await socialRedirect(down.bff, app("/auth/social/github"), "github"))).toBe(
      "/sign-in?error=api_unreachable",
    );
  });
});

describe("social sign-in: the callback", () => {
  const callback = "/auth/callback/github?code=C1&state=S1";

  it("refuses a state that does not match the cookie, without calling the API", async () => {
    const { bff, calls } = scripted({});

    const response = await socialCallback(bff, app(callback, "navree_social=OTHER"), "github");

    expect(location(response)).toBe("/sign-in?error=social_state");
    expect(cookiesOf(response)[0]).toMatch(/^navree_social=; .*Max-Age=0/);
    expect(calls).toEqual([]);
  });

  it("signs in: the tokens go into httpOnly cookies and the browser lands where it started", async () => {
    const { bff, calls } = scripted({
      "POST /api/auth/social/github/callback": () =>
        Response.json({ access_token: "AT1", refresh_token: "RT1", expires_in: 900, return_to: "/w/acme" }),
    });

    const response = await socialCallback(bff, app(callback, "navree_social=S1"), "github");

    expect(response.status).toBe(303);
    expect(location(response)).toBe("/w/acme");
    expect(JSON.parse(calls[0].body)).toEqual({ code: "C1", state: "S1" });
    const cookies = cookiesOf(response);
    expect(cookies).toContain("navree_at=AT1; Path=/; HttpOnly; SameSite=Lax; Max-Age=900");
    expect(cookies).toContain("navree_rt=RT1; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000");
    expect(cookies.some((c) => c.startsWith("navree_social=;"))).toBe(true);
  });

  it("keeps the two-factor challenge in a cookie and continues at /two-factor", async () => {
    const { bff } = scripted({
      "POST /api/auth/social/github/callback": () =>
        Response.json({ two_factor_required: true, challenge_token: "CH1", expires_in: 300, return_to: "/w/acme" }),
    });

    const response = await socialCallback(bff, app(callback, "navree_social=S1"), "github");

    expect(location(response)).toBe("/two-factor?next=%2Fw%2Facme");
    const cookies = cookiesOf(response);
    expect(cookies).toContain("navree_2fa=CH1; Path=/api/auth/2fa; HttpOnly; SameSite=Lax; Max-Age=300");
    expect(cookies.some((c) => c.startsWith("navree_at="))).toBe(false);
  });

  it("goes back to sign-in when the provider's answer is refused, and never follows an outside return_to", async () => {
    const refused = scripted({
      "POST /api/auth/social/github/callback": () => Response.json({ statusCode: 400 }, { status: 400 }),
    });
    const outside = scripted({
      "POST /api/auth/social/github/callback": () =>
        Response.json({ access_token: "AT1", refresh_token: "RT1", return_to: "https://evil.example" }),
    });

    expect(location(await socialCallback(refused.bff, app(callback, "navree_social=S1"), "github"))).toBe(
      "/sign-in?error=social_failed",
    );
    expect(location(await socialCallback(outside.bff, app(callback, "navree_social=S1"), "github"))).toBe("/");
  });
});

describe("the verification link", () => {
  const link = "/api/auth/email/verify/u1/h1?expires=1&signature=sig";

  it("marks the address verified at Navree and lands on /verify-email", async () => {
    const { bff, calls } = scripted({ "GET /api/auth/email/verify/u1/h1": () => new Response(null, { status: 204 }) });

    const response = await verifyEmail(bff, app(link));

    expect(location(response)).toBe("/verify-email?status=verified");
    expect(calls[0].url.search).toBe("?expires=1&signature=sig");
  });

  it("says when the link is no good or the API is down", async () => {
    const invalid = scripted({ "GET /api/auth/email/verify/u1/h1": () => new Response(null, { status: 403 }) });
    const down = scripted({
      "GET /api/auth/email/verify/u1/h1": () => {
        throw new TypeError("fetch failed");
      },
    });

    expect(location(await verifyEmail(invalid.bff, app(link)))).toBe("/verify-email?status=invalid");
    expect(location(await verifyEmail(down.bff, app(link)))).toBe("/verify-email?status=unreachable");
  });
});
