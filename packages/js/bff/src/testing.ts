/**
 * Test helpers for code built on the BFF: a scripted Navree API to hand to
 * `createBff({ fetch })`, browser requests with cookies and an `Origin`, and
 * a `Set-Cookie` reader.
 *
 * ```ts
 * import { createBff } from "@navree/bff";
 * import { browserRequest, fakeNavree, readSetCookies, tokenPair } from "@navree/bff/testing";
 *
 * const navree = fakeNavree({ "POST /api/auth/login": () => Response.json({ user, ...tokenPair(1) }) });
 * const bff = createBff({ apiUrl: "http://api.test", fetch: navree.fetch });
 * const response = await bff.login(browserRequest("http://app.test", "/api/auth/login", { method: "POST", body }));
 * expect(readSetCookies(response).navree_at.value).toBe("AT1");
 * ```
 */

/** One request the fake API received. */
export interface RecordedCall {
  url: string;
  method: string;
  headers: Headers;
  body: string;
}

export type FakeRoute = (call: RecordedCall) => Response | Promise<Response>;

/**
 * A scripted Navree API: routes by `"METHOD /path"`, records every call,
 * and answers 404 in Navree's error shape for anything else.
 */
export function fakeNavree(routes: Record<string, FakeRoute>) {
  const calls: RecordedCall[] = [];
  const fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request ? input : undefined;
    const url = request ? request.url : String(input);
    const method = (init?.method ?? request?.method ?? "GET").toUpperCase();
    const raw = init?.body;
    const body =
      raw === undefined || raw === null
        ? request
          ? await request.clone().text()
          : ""
        : typeof raw === "string"
          ? raw
          : new TextDecoder().decode(raw as ArrayBuffer | Uint8Array);
    const headers = new Headers(request?.headers);
    new Headers(init?.headers).forEach((value, name) => headers.set(name, value));
    const call: RecordedCall = { url, method, headers, body };
    calls.push(call);
    const route = routes[`${method} ${new URL(url).pathname}`];
    if (!route) {
      return Response.json(
        { statusCode: 404, message: "Not Found", error: "Not Found", code: "not_found" },
        { status: 404 },
      );
    }
    return route(call);
  };
  return { fetch: fetch as typeof globalThis.fetch, calls };
}

export interface BrowserRequestOptions {
  method?: string;
  cookies?: Record<string, string>;
  /** The `Origin` header: the app's own by default for non-GET requests; `null` sends none. */
  origin?: string | null;
  body?: unknown;
  headers?: Record<string, string>;
}

/** A request as a browser on `appUrl` sends it to the BFF. */
export function browserRequest(appUrl: string, path: string, options: BrowserRequestOptions = {}): Request {
  const { method = "GET", cookies = {}, origin, body, headers = {} } = options;
  const app = new URL(appUrl);
  const h = new Headers({ host: app.host, ...headers });
  const cookieHeader = Object.entries(cookies)
    .map(([k, v]) => `${k}=${encodeURIComponent(v)}`)
    .join("; ");
  if (cookieHeader) h.set("cookie", cookieHeader);
  const sendOrigin = origin === undefined ? (method === "GET" || method === "HEAD" ? undefined : app.origin) : origin;
  if (sendOrigin) h.set("origin", sendOrigin);
  if (body !== undefined && !h.has("content-type")) h.set("content-type", "application/json");
  return new Request(`${app.origin}${path}`, {
    method,
    headers: h,
    body: body === undefined ? undefined : typeof body === "string" ? body : JSON.stringify(body),
  });
}

/** The `Set-Cookie` headers of a response, by cookie name. */
export function readSetCookies(response: Response | Headers | string[]): Record<string, { value: string; attrs: string[] }> {
  const lines = Array.isArray(response)
    ? response
    : (response instanceof Headers ? response : response.headers).getSetCookie();
  const out: Record<string, { value: string; attrs: string[] }> = {};
  for (const line of lines) {
    const [pair = "", ...attrs] = line.split("; ");
    const eq = pair.indexOf("=");
    out[pair.slice(0, eq)] = { value: decodeURIComponent(pair.slice(eq + 1)), attrs };
  }
  return out;
}

/** Navree's token response, numbered: `AT1`/`RT1`, 15 minutes. */
export function tokenPair(n: number, expiresIn = 900) {
  return { access_token: `AT${n}`, refresh_token: `RT${n}`, token_type: "Bearer", expires_in: expiresIn };
}

/** An unsigned JWT with the given claims, for tests of expiry handling (never accepted by an API). */
export function unsignedJwt(claims: Record<string, unknown>): string {
  const encode = (value: unknown) =>
    btoa(JSON.stringify(value)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `${encode({ alg: "none", typ: "JWT" })}.${encode(claims)}.`;
}
