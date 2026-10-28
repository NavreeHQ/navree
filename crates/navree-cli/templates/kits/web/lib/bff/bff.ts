/**
 * Backend-for-frontend for the Navree API.
 *
 * The browser only ever talks to this Next.js server, same origin, with no
 * tokens. The BFF keeps the JWT access token and the rotating refresh token in
 * httpOnly cookies and forwards requests to Navree with `Authorization: Bearer`
 * (and the `X-Workspace` header the browser's API client sets from the
 * `/w/<slug>/` URL).
 *
 * - `admin` forwards `/admin/*` (the Helm admin panel's pages, forms and
 *   assets) the same way, so the panel opens on this origin with the same
 *   session; without one it sends the browser to the sign-in page;
 * - `socialRedirect` and `socialCallback` run a social sign-in: the state
 *   waits in an httpOnly cookie between the two, and the callback ends like a
 *   password sign-in;
 * - `verifyEmail` follows the signed verification link from the mail and
 *   lands on the web app's `/verify-email` page;
 *
 * - `login` stores both tokens and returns only `{user}`; for an account with
 *   two-factor authentication Navree answers with a challenge instead, which the
 *   BFF keeps in its own httpOnly cookie until `verifyTwoFactor` completes it;
 * - `proxy` forwards any `/api/*` or `/broadcasting/*` request; on a 401 it
 *   refreshes once (single-flight per refresh token), updates the cookies and
 *   retries; if the refresh fails it clears the cookies and returns 401. A token
 *   pair in an `/api/auth/*` response (e.g. `PATCH /api/auth/me/password`)
 *   replaces the cookies and is stripped from the body;
 * - `logout` revokes the refresh token at Navree and clears the cookies;
 * - every non-GET request must come from the app's own origin (403 otherwise),
 *   which is the CSRF defence for the cookie hop;
 * - the browser's `X-Forwarded-For` / `X-Real-IP` never reach Navree; the BFF
 *   sends its own `X-Forwarded-For` only when it knows the client address
 *   (see `trustedProxyHops`).
 *
 * Kept free of `next/headers` so it can be unit-tested with plain Requests.
 */
import { isIP } from "node:net";
import { errorResponse } from "./http-errors";

export const ACCESS_COOKIE = "navree_at";
export const REFRESH_COOKIE = "navree_rt";
/** The two-factor challenge token between the password step and the code step. */
export const CHALLENGE_COOKIE = "navree_2fa";
/** Only the code step's route handler receives the challenge cookie. */
const CHALLENGE_PATH = "/api/auth/2fa";

/** Navree's token response (`POST /api/auth/login` and `/api/auth/refresh`). */
export interface TokenPair {
  access_token: string;
  refresh_token: string;
  token_type?: string;
  /** Access token lifetime in seconds. */
  expires_in?: number;
}

export interface BffOptions {
  /** Navree API base URL, e.g. `http://127.0.0.1:8080`. */
  apiUrl: string;
  /** Fetch used for upstream calls (tests inject one). */
  fetch?: typeof fetch;
  /** `Secure` flag on the token cookies (default: on in production). */
  secureCookies?: boolean;
  /** Extra origins allowed to send non-GET requests (e.g. the public URL behind a proxy). */
  allowedOrigins?: string[];
  /** Refresh-token cookie lifetime in seconds (Navree: 30 days). */
  refreshMaxAge?: number;
  /** How long a completed refresh is remembered for requests still carrying the old cookie. */
  refreshGraceMs?: number;
  /**
   * How many reverse proxies in front of this Next.js server append the address they saw to
   * `X-Forwarded-For` (default 0). Next.js keeps a browser's own `X-Forwarded-For` as is and
   * gives route handlers no other way to read the connection's address, so without a trusted
   * proxy the header is the browser's word and the BFF sends Navree no client address (Navree
   * then sees the BFF's own). With `n` proxies the client is the `n`-th entry from the right:
   * the one the outermost proxy appended. Next.js must then be reachable only through them.
   */
  trustedProxyHops?: number;
  /**
   * A header that the platform or the one proxy in front of this server sets to the bare client
   * address, replacing any value the browser sent (e.g. `x-real-ip` behind a proxy configured to
   * overwrite it, `cf-connecting-ip`, `fly-client-ip`). The BFF believes it before it looks at
   * `X-Forwarded-For`, so it works with a single trusted hop and no `X-Forwarded-For` chain.
   * Set it only when the server is reachable through that proxy alone.
   */
  clientIpHeader?: string;
}

const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

/** Request headers worth passing to Navree. Cookies never leave the BFF. */
const FORWARD_REQUEST_HEADERS = [
  "accept",
  "accept-language",
  "content-type",
  "if-none-match",
  "if-modified-since",
  "user-agent",
  "x-request-id",
  // The workspace of the request (`/w/<slug>/` in the browser).
  "x-workspace",
];

/** The admin panel's responses also carry these (HTML pages, assets and their policy). */
const ADMIN_RESPONSE_HEADERS = ["content-security-policy", "referrer-policy", "x-content-type-options", "x-frame-options"];

/** The social sign-in state, between the redirect and the callback. */
export const SOCIAL_STATE_COOKIE = "navree_social";
/** Only the callback routes receive the state cookie. */
const SOCIAL_CALLBACK_PATH = "/auth/callback";

/** Response headers passed back to the browser (no hop-by-hop or encoding headers: fetch already decoded the body). */
const FORWARD_RESPONSE_HEADERS = [
  "cache-control",
  "content-disposition",
  "content-type",
  "etag",
  "last-modified",
  "location",
  "retry-after",
  "vary",
  "x-request-id",
];

type Tokens = { access?: string; refresh?: string };

/** Navree's login body (`POST /api/auth/login`, `/api/auth/2fa/verify`). */
type LoginBody = Partial<TokenPair> & {
  user?: unknown;
  two_factor_required?: boolean;
  challenge_token?: string;
  expires_in?: number;
};

function isTokenPair(body: unknown): body is TokenPair & { user?: unknown } {
  if (typeof body !== "object" || body === null) return false;
  const b = body as Record<string, unknown>;
  return typeof b.access_token === "string" && typeof b.refresh_token === "string";
}

/**
 * Drops a response we won't forward. Read to the end rather than `body.cancel()`:
 * Next.js's server `fetch` hands out a tee'd stream, and cancelling one branch of
 * a tee never settles while the other branch is open. Only used for small bodies
 * (401s, logout).
 */
async function discard(response: Response | null | undefined): Promise<void> {
  await response?.arrayBuffer().catch(() => undefined);
}

type CookieOptions = { maxAge: number; path?: string };

/** Minimal cookie reader: works for NextRequest and plain Request. */
function readCookie(request: Request, name: string): string | undefined {
  const header = request.headers.get("cookie");
  if (!header) return undefined;
  for (const part of header.split(";")) {
    const eq = part.indexOf("=");
    if (eq !== -1 && part.slice(0, eq).trim() === name) {
      const value = part.slice(eq + 1).trim();
      try {
        return decodeURIComponent(value);
      } catch {
        return value;
      }
    }
  }
  return undefined;
}

export function createBff(options: BffOptions) {
  const apiUrl = options.apiUrl.replace(/\/+$/, "");
  const upstreamFetch = options.fetch ?? ((input: RequestInfo | URL, init?: RequestInit) => fetch(input, init));
  const secure = options.secureCookies ?? process.env.NODE_ENV === "production";
  const refreshMaxAge = options.refreshMaxAge ?? 60 * 60 * 24 * 30;
  const refreshGraceMs = options.refreshGraceMs ?? 30_000;
  const hops = Math.floor(options.trustedProxyHops ?? 0);
  const trustedProxyHops = Number.isFinite(hops) && hops > 0 ? hops : 0;
  const clientIpHeader = options.clientIpHeader?.trim().toLowerCase() || null;

  /**
   * Refresh tokens rotate and reuse revokes the whole family, so concurrent
   * requests holding the same refresh cookie must share one refresh call, and
   * requests that arrive shortly after with the old cookie get the new pair.
   */
  const refreshes = new Map<string, { promise: Promise<TokenPair | null>; settledAt?: number }>();

  function cookie(name: string, value: string, { maxAge, path = "/" }: CookieOptions): string {
    const parts = [
      `${name}=${encodeURIComponent(value)}`,
      `Path=${path}`,
      "HttpOnly",
      "SameSite=Lax",
      `Max-Age=${maxAge}`,
    ];
    if (secure) parts.push("Secure");
    return parts.join("; ");
  }

  function setTokenCookies(headers: Headers, tokens: TokenPair): void {
    const accessMaxAge = Math.max(1, Math.floor(tokens.expires_in ?? 900));
    headers.append("set-cookie", cookie(ACCESS_COOKIE, tokens.access_token, { maxAge: accessMaxAge }));
    headers.append("set-cookie", cookie(REFRESH_COOKIE, tokens.refresh_token, { maxAge: refreshMaxAge }));
  }

  function clearTokenCookies(headers: Headers): void {
    headers.append("set-cookie", cookie(ACCESS_COOKIE, "", { maxAge: 0 }));
    headers.append("set-cookie", cookie(REFRESH_COOKIE, "", { maxAge: 0 }));
  }

  function setChallengeCookie(headers: Headers, token: string, expiresIn: number | undefined): void {
    const maxAge = Math.max(1, Math.floor(expiresIn ?? 300));
    headers.append("set-cookie", cookie(CHALLENGE_COOKIE, token, { maxAge, path: CHALLENGE_PATH }));
  }

  function clearChallengeCookie(headers: Headers): void {
    headers.append("set-cookie", cookie(CHALLENGE_COOKIE, "", { maxAge: 0, path: CHALLENGE_PATH }));
  }

  /** The browser's answer to a successful sign-in: the token cookies plus `{user}`. */
  function signedIn(upstream: Response, data: TokenPair & { user?: unknown }, extra?: (headers: Headers) => void) {
    const headers = new Headers({ "content-type": "application/json" });
    const requestId = upstream.headers.get("x-request-id");
    if (requestId) headers.set("x-request-id", requestId);
    setTokenCookies(headers, data);
    extra?.(headers);
    return new Response(JSON.stringify({ user: data.user }), { status: 200, headers });
  }

  function tokensFrom(request: Request): Tokens {
    return { access: readCookie(request, ACCESS_COOKIE), refresh: readCookie(request, REFRESH_COOKIE) };
  }

  /** Non-GET requests must come from this app: `Origin` host = the host the browser addressed. */
  function originAllowed(request: Request): boolean {
    if (SAFE_METHODS.has(request.method.toUpperCase())) return true;
    const origin = request.headers.get("origin");
    if (!origin || origin === "null") return false;
    if (options.allowedOrigins?.includes(origin)) return true;
    let originHost: string;
    try {
      originHost = new URL(origin).host;
    } catch {
      return false;
    }
    const host = request.headers.get("x-forwarded-host")?.split(",")[0]?.trim() || request.headers.get("host");
    const ownHost = host ?? new URL(request.url).host;
    return originHost === ownHost;
  }

  function upstreamHeaders(request: Request, accessToken?: string): Headers {
    const headers = new Headers();
    for (const name of FORWARD_REQUEST_HEADERS) {
      const value = request.headers.get(name);
      if (value) headers.set(name, value);
    }
    if (!headers.has("accept")) headers.set("accept", "application/json");
    for (const [name, value] of Object.entries(forwardedFor(request))) headers.set(name, value);
    if (accessToken) headers.set("authorization", `Bearer ${accessToken}`);
    return headers;
  }

  /**
   * The client's address as the trusted proxies in front of Next.js recorded it, or `null`
   * when there are none (the browser's own `X-Forwarded-For` is never believed) or their
   * entry is missing or not an IP address.
   */
  function clientAddress(request: Request): string | null {
    if (clientIpHeader) {
      const value = request.headers.get(clientIpHeader)?.trim();
      if (value && isIP(value)) return value;
    }
    if (trustedProxyHops === 0) return null;
    const chain = (request.headers.get("x-forwarded-for") ?? "").split(",").map((entry) => entry.trim());
    const entry = chain.length >= trustedProxyHops ? chain[chain.length - trustedProxyHops] : undefined;
    return entry && isIP(entry) ? entry : null;
  }

  /**
   * `X-Forwarded-For` for Navree, built by the BFF: a single entry, so Navree (which takes the
   * right-most entry when `http.trust_proxy` is on) sees the client address the BFF vouches for.
   */
  function forwardedFor(request: Request): Record<string, string> {
    const address = clientAddress(request);
    return address ? { "x-forwarded-for": address } : {};
  }

  async function callNavreeRefresh(refreshToken: string, request: Request): Promise<TokenPair | null> {
    try {
      const response = await upstreamFetch(`${apiUrl}/api/auth/refresh`, {
        method: "POST",
        cache: "no-store",
        headers: { "content-type": "application/json", accept: "application/json", ...forwardedFor(request) },
        body: JSON.stringify({ refresh_token: refreshToken }),
      });
      if (!response.ok) {
        await discard(response);
        return null;
      }
      const pair = (await response.json()) as TokenPair;
      return pair.access_token && pair.refresh_token ? pair : null;
    } catch {
      return null;
    }
  }

  /** Single-flight refresh keyed by the refresh token being spent. */
  function refresh(refreshToken: string, request: Request): Promise<TokenPair | null> {
    const now = Date.now();
    for (const [key, entry] of refreshes) {
      if (entry.settledAt !== undefined && now - entry.settledAt > refreshGraceMs) refreshes.delete(key);
    }
    const existing = refreshes.get(refreshToken);
    if (existing) return existing.promise;
    const entry: { promise: Promise<TokenPair | null>; settledAt?: number } = {
      promise: callNavreeRefresh(refreshToken, request).then((pair) => {
        entry.settledAt = Date.now();
        if (!pair) refreshes.delete(refreshToken);
        return pair;
      }),
    };
    refreshes.set(refreshToken, entry);
    return entry.promise;
  }

  /** Streams an upstream response back with its status, body and selected headers. */
  function passThrough(upstream: Response, extraHeaders?: Headers): Response {
    const headers = new Headers();
    for (const name of FORWARD_RESPONSE_HEADERS) {
      const value = upstream.headers.get(name);
      if (value) headers.set(name, value);
    }
    for (const [name, value] of upstream.headers) {
      if (name.startsWith("x-ratelimit-")) headers.set(name, value);
    }
    extraHeaders?.forEach((value, name) => headers.append(name, value));
    const bodyless = upstream.status === 204 || upstream.status === 304 || upstream.status < 200;
    return new Response(bodyless ? null : upstream.body, {
      status: upstream.status,
      statusText: upstream.statusText,
      headers,
    });
  }

  function unreachable(): Response {
    return errorResponse(502, "api_unreachable", "The Navree API is unreachable");
  }

  /**
   * Forwards a request to Navree at `upstreamPath` (+ the request's query) with
   * the bearer token, refreshing once on a 401.
   */
  async function proxy(request: Request, upstreamPath: string): Promise<Response> {
    if (!originAllowed(request)) return errorResponse(403, "invalid_origin", "Invalid origin");

    const method = request.method.toUpperCase();
    // Buffered so the request can be replayed after a refresh. API bodies are small JSON documents.
    const body = method === "GET" || method === "HEAD" ? undefined : await request.arrayBuffer();
    const url = `${apiUrl}${upstreamPath}${new URL(request.url).search}`;
    const send = (accessToken?: string) =>
      upstreamFetch(url, {
        method,
        headers: upstreamHeaders(request, accessToken),
        body,
        redirect: "manual",
        // Authenticated responses must never land in Next's fetch cache.
        cache: "no-store",
      });

    const tokens = tokensFrom(request);
    let refreshed: TokenPair | null = null;
    let refreshFailed = false;

    // The access cookie expired (or was never set) but a refresh token remains: refresh up front.
    if (!tokens.access && tokens.refresh) {
      refreshed = await refresh(tokens.refresh, request);
      refreshFailed = refreshed === null;
    }

    let upstream: Response;
    try {
      upstream = await send(refreshed?.access_token ?? tokens.access);
      if (upstream.status === 401 && !refreshed && tokens.refresh && !refreshFailed) {
        refreshed = await refresh(tokens.refresh, request);
        if (refreshed) {
          await discard(upstream);
          upstream = await send(refreshed.access_token);
        } else {
          refreshFailed = true;
        }
      }
    } catch {
      return unreachable();
    }

    // New pair → set both cookies. Refresh failed, or Navree rejected the token with nothing to
    // refresh it with → the session is over: clear both so the browser goes back to /login.
    const cookies = new Headers();
    if (refreshed) setTokenCookies(cookies, refreshed);
    else if (refreshFailed || (upstream.status === 401 && tokens.access)) clearTokenCookies(cookies);
    if (upstream.ok && upstreamPath.startsWith("/api/auth/") && isJson(upstream)) {
      return withoutTokens(upstream, cookies);
    }
    return passThrough(upstream, cookies);
  }

  function isJson(response: Response): boolean {
    return (response.headers.get("content-type") ?? "").includes("json");
  }

  /**
   * Account routes that issue tokens (`PATCH /api/auth/me/password` returns a new pair,
   * `POST /api/auth/2fa/verify` the login body): the pair goes into the cookies and
   * never into the browser; the body keeps only `user` (or becomes a 204).
   */
  async function withoutTokens(upstream: Response, cookies: Headers): Promise<Response> {
    const text = await upstream.text();
    let body: unknown;
    try {
      body = JSON.parse(text);
    } catch {
      body = undefined;
    }
    if (!isTokenPair(body)) {
      return passThrough(new Response(text, { status: upstream.status, headers: upstream.headers }), cookies);
    }
    const headers = new Headers();
    setTokenCookies(headers, body);
    const requestId = upstream.headers.get("x-request-id");
    if (requestId) headers.set("x-request-id", requestId);
    if (body.user === undefined) return new Response(null, { status: 204, headers });
    headers.set("content-type", "application/json");
    return new Response(JSON.stringify({ user: body.user }), { status: 200, headers });
  }

  /** `POST /api/auth/login`: keeps the tokens, returns `{user}`. */
  async function login(request: Request): Promise<Response> {
    if (!originAllowed(request)) return errorResponse(403, "invalid_origin", "Invalid origin");
    let upstream: Response;
    try {
      upstream = await upstreamFetch(`${apiUrl}/api/auth/login`, {
        method: "POST",
        cache: "no-store",
        headers: upstreamHeaders(request),
        body: await request.arrayBuffer(),
      });
    } catch {
      return unreachable();
    }
    if (!upstream.ok) return passThrough(upstream);

    const data = (await upstream.json().catch(() => ({}))) as LoginBody;
    if (data.two_factor_required === true && typeof data.challenge_token === "string") {
      // Two-factor account: no session yet. The challenge token waits in a cookie only the code step receives.
      const headers = new Headers({ "content-type": "application/json" });
      setChallengeCookie(headers, data.challenge_token, data.expires_in);
      const body = { two_factor_required: true, expires_in: data.expires_in ?? 300 };
      return new Response(JSON.stringify(body), { status: 200, headers });
    }
    if (!isTokenPair(data))
      return errorResponse(502, "unexpected_api_response", "Unexpected login response from the API");
    return signedIn(upstream, data);
  }

  /**
   * `POST /api/auth/2fa/verify` (`{code}` or `{recovery_code}`): completes a two-factor
   * sign-in with the challenge token from the cookie, then stores the tokens like `login`.
   */
  async function verifyTwoFactor(request: Request): Promise<Response> {
    if (!originAllowed(request)) return errorResponse(403, "invalid_origin", "Invalid origin");
    const challengeToken = readCookie(request, CHALLENGE_COOKIE);
    if (!challengeToken)
      return errorResponse(401, "two_factor_challenge_expired", "Your sign-in attempt expired. Sign in again.");

    let input: Record<string, unknown> = {};
    try {
      input = ((await request.json()) as Record<string, unknown> | null) ?? {};
    } catch {
      // no body: Navree answers 400
    }
    const proof: Record<string, string> = {};
    if (typeof input.code === "string") proof.code = input.code;
    else if (typeof input.recovery_code === "string") proof.recovery_code = input.recovery_code;

    let upstream: Response;
    try {
      upstream = await upstreamFetch(`${apiUrl}/api/auth/2fa/verify`, {
        method: "POST",
        cache: "no-store",
        headers: { ...Object.fromEntries(upstreamHeaders(request)), "content-type": "application/json" },
        body: JSON.stringify({ challenge_token: challengeToken, ...proof }),
      });
    } catch {
      return unreachable();
    }
    // A wrong code keeps the challenge cookie, so the person can try again until it expires.
    if (!upstream.ok) return passThrough(upstream);
    const data = (await upstream.json().catch(() => ({}))) as LoginBody;
    if (!isTokenPair(data)) return errorResponse(502, "unexpected_api_response", "Unexpected response from the API");
    return signedIn(upstream, data, clearChallengeCookie);
  }

  /** `POST /api/auth/logout` (`{all?: boolean}`): revokes the refresh token at Navree, clears the cookies. */
  async function logout(request: Request): Promise<Response> {
    if (!originAllowed(request)) return errorResponse(403, "invalid_origin", "Invalid origin");
    let all = false;
    try {
      const body = (await request.json()) as { all?: unknown };
      all = body?.all === true;
    } catch {
      // no body
    }
    const tokens = tokensFrom(request);
    const revoke = (access: string | undefined, refreshToken: string | undefined) =>
      upstreamFetch(`${apiUrl}/api/auth/logout`, {
        method: "POST",
        cache: "no-store",
        headers: { ...Object.fromEntries(upstreamHeaders(request, access)), "content-type": "application/json" },
        body: JSON.stringify({
          ...(refreshToken ? { refresh_token: refreshToken } : {}),
          ...(all ? { all: true } : {}),
        }),
      });

    try {
      if (tokens.access || tokens.refresh) {
        let response = tokens.access ? await revoke(tokens.access, tokens.refresh) : null;
        if ((!response || response.status === 401) && tokens.refresh) {
          // Logout needs a bearer token: get a fresh pair, then revoke the new refresh token.
          await discard(response);
          const pair = await refresh(tokens.refresh, request);
          response = pair ? await revoke(pair.access_token, pair.refresh_token) : null;
        }
        await discard(response);
      }
    } catch {
      // Navree unreachable: still sign the browser out.
    }
    const headers = new Headers();
    clearTokenCookies(headers);
    return new Response(null, { status: 204, headers });
  }

  /** `POST /api/auth/refresh` from the browser: rotates the cookies (204) or ends the session (401). */
  async function refreshSession(request: Request): Promise<Response> {
    if (!originAllowed(request)) return errorResponse(403, "invalid_origin", "Invalid origin");
    const tokens = tokensFrom(request);
    const pair = tokens.refresh ? await refresh(tokens.refresh, request) : null;
    const headers = new Headers();
    if (!pair) {
      clearTokenCookies(headers);
      return errorResponse(401, "session_expired", "Your session has expired", headers);
    }
    setTokenCookies(headers, pair);
    return new Response(null, { status: 204, headers });
  }

  /** A `303 See Other` to `location` on this origin, with `headers` (cookies). */
  function redirect(location: string, headers?: Headers): Response {
    const out = new Headers(headers);
    out.set("location", location);
    return new Response(null, { status: 303, headers: out });
  }

  /** `next` when it is a path on this origin, else `/` (never an open redirect). */
  function localPath(next: string | null | undefined): string {
    return next && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/\\") ? next : "/";
  }

  /** Where the sign-in page sends the browser back to. */
  function signIn(request: Request, headers?: Headers): Response {
    const url = new URL(request.url);
    return redirect(`/sign-in?next=${encodeURIComponent(url.pathname + url.search)}`, headers);
  }

  /**
   * `/admin/*`: the Helm admin panel, forwarded with the bearer token like `proxy` (refreshing
   * once on a 401). Pages, assets, form posts and redirects pass through with the panel's
   * security headers; a browser without a session, or whose session ended, goes to the sign-in
   * page and comes back.
   */
  async function admin(request: Request): Promise<Response> {
    if (!originAllowed(request)) return errorResponse(403, "invalid_origin", "Invalid origin");
    const tokens = tokensFrom(request);
    if (!tokens.access && !tokens.refresh) return signIn(request);

    const url = new URL(request.url);
    const method = request.method.toUpperCase();
    const body = method === "GET" || method === "HEAD" ? undefined : await request.arrayBuffer();
    const send = (accessToken?: string) => {
      const headers = upstreamHeaders(request, accessToken);
      headers.set("accept", request.headers.get("accept") ?? "text/html");
      return upstreamFetch(`${apiUrl}${url.pathname}${url.search}`, {
        method,
        headers,
        body,
        redirect: "manual",
        cache: "no-store",
      });
    };

    let refreshed: TokenPair | null = null;
    if (!tokens.access && tokens.refresh) refreshed = await refresh(tokens.refresh, request);
    let upstream: Response;
    try {
      upstream = await send(refreshed?.access_token ?? tokens.access);
      if (upstream.status === 401 && !refreshed && tokens.refresh) {
        refreshed = await refresh(tokens.refresh, request);
        if (refreshed) {
          await discard(upstream);
          upstream = await send(refreshed.access_token);
        }
      }
    } catch {
      return unreachable();
    }

    const cookies = new Headers();
    if (refreshed) setTokenCookies(cookies, refreshed);
    if (upstream.status === 401) {
      await discard(upstream);
      clearTokenCookies(cookies);
      return signIn(request, cookies);
    }
    const response = passThrough(upstream, cookies);
    for (const name of ADMIN_RESPONSE_HEADERS) {
      const value = upstream.headers.get(name);
      if (value) response.headers.set(name, value);
    }
    // A redirect to the API's own address stays on this origin.
    const location = upstream.headers.get("location");
    if (location?.startsWith(apiUrl)) response.headers.set("location", location.slice(apiUrl.length) || "/");
    return response;
  }

  /**
   * `GET /auth/social/{provider}?next=/w/acme`: starts a social sign-in. Navree answers with the
   * provider's URL and a sealed state; the state waits in an httpOnly cookie that only the
   * callback route receives, and the browser goes to the provider.
   */
  async function socialRedirect(request: Request, provider: string): Promise<Response> {
    const next = localPath(new URL(request.url).searchParams.get("next"));
    let upstream: Response;
    try {
      upstream = await upstreamFetch(
        `${apiUrl}/api/auth/social/${encodeURIComponent(provider)}/redirect?return_to=${encodeURIComponent(next)}`,
        { headers: upstreamHeaders(request), cache: "no-store" },
      );
    } catch {
      return redirect("/sign-in?error=api_unreachable");
    }
    if (!upstream.ok) {
      await discard(upstream);
      return redirect("/sign-in?error=social_unavailable");
    }
    const { url, state } = (await upstream.json().catch(() => ({}))) as { url?: string; state?: string };
    if (!url || !state) return redirect("/sign-in?error=social_unavailable");
    const headers = new Headers();
    headers.append("set-cookie", cookie(SOCIAL_STATE_COOKIE, state, { maxAge: 600, path: SOCIAL_CALLBACK_PATH }));
    headers.set("location", url);
    return new Response(null, { status: 303, headers });
  }

  /**
   * `GET /auth/callback/{provider}?code&state`: the provider sends the browser back here. The
   * state must match the cookie; Navree exchanges the code and answers like a password sign-in
   * (tokens into the cookies, then the page the sign-in started from) or with a two-factor
   * challenge (the `/two-factor` page finishes it).
   */
  async function socialCallback(request: Request, provider: string): Promise<Response> {
    const params = new URL(request.url).searchParams;
    const code = params.get("code");
    const state = params.get("state");
    const expected = readCookie(request, SOCIAL_STATE_COOKIE);
    const headers = new Headers();
    headers.append("set-cookie", cookie(SOCIAL_STATE_COOKIE, "", { maxAge: 0, path: SOCIAL_CALLBACK_PATH }));
    if (!code || !state || !expected || state !== expected) return redirect("/sign-in?error=social_state", headers);

    let upstream: Response;
    try {
      upstream = await upstreamFetch(`${apiUrl}/api/auth/social/${encodeURIComponent(provider)}/callback`, {
        method: "POST",
        cache: "no-store",
        headers: { ...Object.fromEntries(upstreamHeaders(request)), "content-type": "application/json" },
        body: JSON.stringify({ code, state }),
      });
    } catch {
      return redirect("/sign-in?error=api_unreachable", headers);
    }
    if (!upstream.ok) {
      await discard(upstream);
      return redirect("/sign-in?error=social_failed", headers);
    }
    const data = (await upstream.json().catch(() => ({}))) as LoginBody & { return_to?: string };
    const next = localPath(data.return_to);
    if (data.two_factor_required === true && typeof data.challenge_token === "string") {
      setChallengeCookie(headers, data.challenge_token, data.expires_in);
      return redirect(`/two-factor?next=${encodeURIComponent(next)}`, headers);
    }
    if (!isTokenPair(data)) return redirect("/sign-in?error=social_failed", headers);
    setTokenCookies(headers, data);
    return redirect(next, headers);
  }

  /**
   * `GET /api/auth/email/verify/{id}/{hash}?expires&signature`: the link in the verification
   * mail. Navree checks the signature and marks the address verified; the browser lands on
   * `/verify-email?status=verified` (or `invalid`).
   */
  async function verifyEmail(request: Request): Promise<Response> {
    const url = new URL(request.url);
    try {
      const upstream = await upstreamFetch(`${apiUrl}${url.pathname}${url.search}`, {
        headers: upstreamHeaders(request),
        cache: "no-store",
      });
      await discard(upstream);
      return redirect(`/verify-email?status=${upstream.ok ? "verified" : "invalid"}`);
    } catch {
      return redirect("/verify-email?status=unreachable");
    }
  }

  return {
    proxy,
    admin,
    login,
    verifyTwoFactor,
    logout,
    refreshSession,
    socialRedirect,
    socialCallback,
    verifyEmail,
    originAllowed,
  };
}

export type Bff = ReturnType<typeof createBff>;
