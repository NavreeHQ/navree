/**
 * The backend-for-frontend for a Navree API.
 *
 * The browser only ever talks to the app's own server, same origin, with no
 * tokens. The BFF keeps the JWT access token and the rotating refresh token in
 * httpOnly cookies and forwards requests to Navree with `Authorization: Bearer`.
 *
 * - `login` stores both tokens and returns only `{user}`; for an account with
 *   two-factor authentication Navree answers with a challenge instead, which the
 *   BFF keeps in its own httpOnly cookie until `verifyTwoFactor` completes it;
 * - `proxy` forwards any request (`/api/*`, `/broadcasting/auth`, a server-rendered
 *   tool under `/admin`); on a 401 it refreshes once (single-flight per refresh
 *   token), updates the cookies and retries; if the refresh fails it clears the
 *   cookies and returns 401. A token pair in an auth response (e.g.
 *   `PATCH /api/auth/me/password`) replaces the cookies and is stripped from the body;
 * - `logout` revokes the refresh token at Navree and clears the cookies;
 * - `handle` routes a request to the right one of these by its path;
 * - `ensureSession` gives middleware and server rendering a valid access token,
 *   refreshing it when it has expired, and `api`/`me` call Navree with it;
 * - every non-GET request must come from the app's own origin (403 otherwise),
 *   which is the CSRF defence for the cookie hop;
 * - the browser's `X-Forwarded-For` / `X-Real-IP` never reach Navree; the BFF
 *   sends its own `X-Forwarded-For` only when it knows the client address
 *   (see `trustedProxyHops`).
 *
 * Web-standard `Request` in, `Response` out: no framework imports, so Next.js
 * route handlers, Nitro handlers and tests all use it as is.
 */
import { isIP } from "node:net";
import { errorResponse } from "./http-errors.js";

export const ACCESS_COOKIE = "navree_at";
export const REFRESH_COOKIE = "navree_rt";
/** The two-factor challenge token between the password step and the code step. */
export const CHALLENGE_COOKIE = "navree_2fa";
/** The current workspace (a slug or an id). Not a secret: the browser sets it when the user switches. */
export const WORKSPACE_COOKIE = "navree_workspace";

/** Navree's token response (`POST /api/auth/login` and `/api/auth/refresh`). */
export interface TokenPair {
  access_token: string;
  refresh_token: string;
  token_type?: string;
  /** Access token lifetime in seconds. */
  expires_in?: number;
}

export interface CookieNames {
  access?: string;
  refresh?: string;
  challenge?: string;
  workspace?: string;
}

export interface BffOptions {
  /** Navree API base URL, e.g. `http://127.0.0.1:8080`. */
  apiUrl: string;
  /** Fetch used for upstream calls (tests inject one). */
  fetch?: typeof fetch;
  /** `Secure` flag on the cookies (default: on in production). */
  secureCookies?: boolean;
  /** Extra origins allowed to send non-GET requests (e.g. the public URL behind a proxy). */
  allowedOrigins?: string[];
  /** Refresh-token cookie lifetime in seconds (Navree: 30 days). */
  refreshMaxAge?: number;
  /** How long a completed refresh is remembered for requests still carrying the old cookie. */
  refreshGraceMs?: number;
  /**
   * How many reverse proxies in front of this server append the address they saw to
   * `X-Forwarded-For` (default 0). Next.js and Nitro keep a browser's own `X-Forwarded-For`
   * as is, so without a trusted proxy the header is the browser's word and the BFF sends
   * Navree no client address (Navree then sees the BFF's own). With `n` proxies the client
   * is the `n`-th entry from the right: the one the outermost proxy appended. The server
   * must then be reachable only through them.
   */
  trustedProxyHops?: number;
  /** The API's auth routes (default `/api/auth`): login, refresh, logout, `2fa/verify`. */
  authPrefix?: string;
  /** The API's current-user route (default `/api/auth/me`). */
  mePath?: string;
  /** The workspace header, as the API's `workspace.header` names it (default `X-Workspace`). */
  workspaceHeader?: string;
  /** Cookie names, for several apps on one domain (defaults `navree_at`, `navree_rt`, `navree_2fa`, `navree_workspace`). */
  cookieNames?: CookieNames;
  /** Maps a request path to the API path `handle` forwards to (default: the same path). */
  upstreamPath?: (pathname: string) => string;
  /** Refresh an access token this many seconds before it expires (`ensureSession`; default 30). */
  refreshSkewSeconds?: number;
}

/** What `ensureSession` found. */
export interface Session {
  /**
   * `anonymous`: no token cookies; `active`: the access token is usable;
   * `refreshed`: it had expired and was refreshed; `expired`: the refresh failed.
   */
  status: "anonymous" | "active" | "refreshed" | "expired";
  /** The access token to call Navree with (`active` and `refreshed`). */
  accessToken?: string;
  /** `Set-Cookie` values the response must carry (new tokens, or cleared ones). */
  setCookie: string[];
  /** After a refresh: the request's `Cookie` header with the new tokens, for the rest of this request. */
  cookieHeader?: string;
}

const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

/** Request headers worth passing to Navree. Cookies never leave the BFF. */
const FORWARD_REQUEST_HEADERS = [
  "accept",
  "accept-language",
  "content-type",
  "if-match",
  "if-none-match",
  "if-modified-since",
  "last-event-id",
  "user-agent",
  "x-request-id",
];

/** Response headers passed back to the browser (no hop-by-hop or encoding headers: fetch already decoded the body). */
const FORWARD_RESPONSE_HEADERS = [
  "cache-control",
  "content-disposition",
  "content-language",
  "content-security-policy",
  "content-type",
  "etag",
  "last-modified",
  "location",
  "referrer-policy",
  "retry-after",
  "vary",
  "x-content-type-options",
  "x-frame-options",
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
 * a server `fetch` (Next.js's) may hand out a tee'd stream, and cancelling one
 * branch of a tee never settles while the other branch is open. Only used for
 * small bodies (401s, logout).
 */
async function discard(response: Response | null | undefined): Promise<void> {
  await response?.arrayBuffer().catch(() => undefined);
}

type CookieOptions = { maxAge: number; path?: string; httpOnly?: boolean };

/** Minimal cookie reader for any web Request. */
export function readCookie(request: Request, name: string): string | undefined {
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

/** `cookieHeader` with `values` replacing (or added after) the cookies of the same names. */
function replaceCookies(cookieHeader: string | null, values: Record<string, string>): string {
  const pending = new Map(Object.entries(values));
  const parts: string[] = [];
  for (const part of (cookieHeader ?? "").split(";")) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf("=");
    const name = eq === -1 ? trimmed : trimmed.slice(0, eq);
    if (pending.has(name)) {
      parts.push(`${name}=${encodeURIComponent(pending.get(name) as string)}`);
      pending.delete(name);
    } else {
      parts.push(trimmed);
    }
  }
  for (const [name, value] of pending) parts.push(`${name}=${encodeURIComponent(value)}`);
  return parts.join("; ");
}

function base64UrlDecode(segment: string): string {
  const base64 = segment.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  return atob(padded);
}

/**
 * The `exp` claim of a JWT access token (seconds since the epoch), read
 * without verifying the signature: only to decide whether to refresh before
 * calling the API, which verifies the token itself. `undefined` for an
 * opaque token or one without `exp`.
 */
export function tokenExpiresAt(token: string): number | undefined {
  const parts = token.split(".");
  if (parts.length !== 3 || !parts[1]) return undefined;
  try {
    const claims = JSON.parse(base64UrlDecode(parts[1])) as { exp?: unknown };
    return typeof claims.exp === "number" ? claims.exp : undefined;
  } catch {
    return undefined;
  }
}

export function createBff(options: BffOptions) {
  const apiUrl = options.apiUrl.replace(/\/+$/, "");
  const upstreamFetch = options.fetch ?? ((input: RequestInfo | URL, init?: RequestInit) => fetch(input, init));
  const secure = options.secureCookies ?? process.env.NODE_ENV === "production";
  const refreshMaxAge = options.refreshMaxAge ?? 60 * 60 * 24 * 30;
  const refreshGraceMs = options.refreshGraceMs ?? 30_000;
  const hops = Math.floor(options.trustedProxyHops ?? 0);
  const trustedProxyHops = Number.isFinite(hops) && hops > 0 ? hops : 0;
  const authPrefix = (options.authPrefix ?? "/api/auth").replace(/\/+$/, "");
  const challengePath = `${authPrefix}/2fa`;
  const mePath = options.mePath ?? `${authPrefix}/me`;
  const workspaceHeader = (options.workspaceHeader ?? "X-Workspace").toLowerCase();
  const names = {
    access: options.cookieNames?.access ?? ACCESS_COOKIE,
    refresh: options.cookieNames?.refresh ?? REFRESH_COOKIE,
    challenge: options.cookieNames?.challenge ?? CHALLENGE_COOKIE,
    workspace: options.cookieNames?.workspace ?? WORKSPACE_COOKIE,
  };
  const upstreamPathOf = options.upstreamPath ?? ((pathname: string) => pathname);
  const refreshSkew = options.refreshSkewSeconds ?? 30;

  /**
   * Refresh tokens rotate and reuse revokes the whole family, so concurrent
   * requests holding the same refresh cookie must share one refresh call, and
   * requests that arrive shortly after with the old cookie get the new pair.
   */
  const refreshes = new Map<string, { promise: Promise<TokenPair | null>; settledAt?: number }>();

  function cookie(name: string, value: string, { maxAge, path = "/", httpOnly = true }: CookieOptions): string {
    const parts = [`${name}=${encodeURIComponent(value)}`, `Path=${path}`];
    if (httpOnly) parts.push("HttpOnly");
    parts.push("SameSite=Lax", `Max-Age=${maxAge}`);
    if (secure) parts.push("Secure");
    return parts.join("; ");
  }

  function tokenCookies(tokens: TokenPair): string[] {
    const accessMaxAge = Math.max(1, Math.floor(tokens.expires_in ?? 900));
    return [
      cookie(names.access, tokens.access_token, { maxAge: accessMaxAge }),
      cookie(names.refresh, tokens.refresh_token, { maxAge: refreshMaxAge }),
    ];
  }

  function clearedTokenCookies(): string[] {
    return [cookie(names.access, "", { maxAge: 0 }), cookie(names.refresh, "", { maxAge: 0 })];
  }

  function setTokenCookies(headers: Headers, tokens: TokenPair): void {
    for (const value of tokenCookies(tokens)) headers.append("set-cookie", value);
  }

  function clearTokenCookies(headers: Headers): void {
    for (const value of clearedTokenCookies()) headers.append("set-cookie", value);
  }

  function setChallengeCookie(headers: Headers, token: string, expiresIn: number | undefined): void {
    const maxAge = Math.max(1, Math.floor(expiresIn ?? 300));
    headers.append("set-cookie", cookie(names.challenge, token, { maxAge, path: challengePath }));
  }

  function clearChallengeCookie(headers: Headers): void {
    headers.append("set-cookie", cookie(names.challenge, "", { maxAge: 0, path: challengePath }));
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
    return { access: readCookie(request, names.access), refresh: readCookie(request, names.refresh) };
  }

  /** The workspace the request is for: its workspace header, else the workspace cookie. */
  function workspaceOf(request: Request): string | undefined {
    return request.headers.get(workspaceHeader)?.trim() || readCookie(request, names.workspace) || undefined;
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

  function invalidOrigin(): Response {
    return errorResponse(403, "Invalid origin", "invalid_origin");
  }

  function upstreamHeaders(request: Request, accessToken?: string): Headers {
    const headers = new Headers();
    for (const name of FORWARD_REQUEST_HEADERS) {
      const value = request.headers.get(name);
      if (value) headers.set(name, value);
    }
    if (!headers.has("accept")) headers.set("accept", "application/json");
    const workspace = workspaceOf(request);
    if (workspace) headers.set(workspaceHeader, workspace);
    for (const [name, value] of Object.entries(forwardedFor(request))) headers.set(name, value);
    if (accessToken) headers.set("authorization", `Bearer ${accessToken}`);
    return headers;
  }

  /**
   * The client's address as the trusted proxies in front of this server recorded it, or
   * `null` when there are none (the browser's own `X-Forwarded-For` is never believed) or
   * their entry is missing or not an IP address.
   */
  function clientAddress(request: Request): string | null {
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
      const response = await upstreamFetch(`${apiUrl}${authPrefix}/refresh`, {
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
    return errorResponse(502, "The Navree API is unreachable", "api_unreachable");
  }

  /**
   * Forwards a request to Navree at `upstreamPath` (+ the request's query) with
   * the bearer token, refreshing once on a 401.
   */
  async function proxy(request: Request, upstreamPath: string): Promise<Response> {
    if (!originAllowed(request)) return invalidOrigin();

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
        // Authenticated responses must never land in a fetch cache.
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
    // refresh it with → the session is over: clear both so the browser goes back to sign-in.
    const cookies = new Headers();
    if (refreshed) setTokenCookies(cookies, refreshed);
    else if (refreshFailed || (upstream.status === 401 && tokens.access)) clearTokenCookies(cookies);
    if (upstream.ok && upstreamPath.startsWith(`${authPrefix}/`) && isJson(upstream)) {
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
    if (!originAllowed(request)) return invalidOrigin();
    let upstream: Response;
    try {
      upstream = await upstreamFetch(`${apiUrl}${authPrefix}/login`, {
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
    if (!isTokenPair(data)) return errorResponse(502, "Unexpected login response from the API", "unexpected_response");
    return signedIn(upstream, data);
  }

  /**
   * `POST /api/auth/2fa/verify` (`{code}` or `{recovery_code}`): completes a two-factor
   * sign-in with the challenge token from the cookie, then stores the tokens like `login`.
   */
  async function verifyTwoFactor(request: Request): Promise<Response> {
    if (!originAllowed(request)) return invalidOrigin();
    const challengeToken = readCookie(request, names.challenge);
    if (!challengeToken) {
      return errorResponse(401, "Your sign-in attempt expired. Sign in again.", "two_factor_expired");
    }

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
      upstream = await upstreamFetch(`${apiUrl}${challengePath}/verify`, {
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
    if (!isTokenPair(data)) return errorResponse(502, "Unexpected response from the API", "unexpected_response");
    return signedIn(upstream, data, clearChallengeCookie);
  }

  /** `POST /api/auth/logout` (`{all?: boolean}`): revokes the refresh token at Navree, clears the cookies. */
  async function logout(request: Request): Promise<Response> {
    if (!originAllowed(request)) return invalidOrigin();
    let all = false;
    try {
      const body = (await request.json()) as { all?: unknown };
      all = body?.all === true;
    } catch {
      // no body
    }
    const tokens = tokensFrom(request);
    const revoke = (access: string | undefined, refreshToken: string | undefined) =>
      upstreamFetch(`${apiUrl}${authPrefix}/logout`, {
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
    if (!originAllowed(request)) return invalidOrigin();
    const tokens = tokensFrom(request);
    const pair = tokens.refresh ? await refresh(tokens.refresh, request) : null;
    const headers = new Headers();
    if (!pair) {
      clearTokenCookies(headers);
      return errorResponse(401, "Your session has expired", "session_expired", headers);
    }
    setTokenCookies(headers, pair);
    return new Response(null, { status: 204, headers });
  }

  /**
   * One entry point for a catch-all route: `POST {authPrefix}/login`, `/2fa/verify`,
   * `/logout` and `/refresh` go to their handlers; everything else is proxied to the
   * same path (or `upstreamPath(pathname)`).
   */
  function handle(request: Request): Promise<Response> {
    const pathname = new URL(request.url).pathname.replace(/\/+$/, "") || "/";
    const upstream = upstreamPathOf(pathname);
    if (request.method.toUpperCase() === "POST") {
      switch (upstream) {
        case `${authPrefix}/login`:
          return login(request);
        case `${challengePath}/verify`:
          return verifyTwoFactor(request);
        case `${authPrefix}/logout`:
          return logout(request);
        case `${authPrefix}/refresh`:
          return refreshSession(request);
      }
    }
    return proxy(request, upstream);
  }

  /**
   * A usable access token for server-side work on this request (middleware,
   * server components, Nitro routes): the cookie's token while it is valid,
   * else a refreshed one. Never throws; a failed refresh is `expired`.
   */
  async function ensureSession(request: Request): Promise<Session> {
    const tokens = tokensFrom(request);
    if (!tokens.access && !tokens.refresh) return { status: "anonymous", setCookie: [] };
    if (tokens.access) {
      const exp = tokenExpiresAt(tokens.access);
      const fresh = exp === undefined || exp - refreshSkew > Date.now() / 1000;
      if (fresh || !tokens.refresh) return { status: "active", accessToken: tokens.access, setCookie: [] };
    }
    const pair = await refresh(tokens.refresh as string, request);
    if (!pair) return { status: "expired", setCookie: clearedTokenCookies() };
    return {
      status: "refreshed",
      accessToken: pair.access_token,
      setCookie: tokenCookies(pair),
      cookieHeader: replaceCookies(request.headers.get("cookie"), {
        [names.access]: pair.access_token,
        [names.refresh]: pair.refresh_token,
      }),
    };
  }

  /**
   * Calls Navree from the server with a session's token (no refresh: get the
   * token from `ensureSession`). `path` is the API path with its query.
   */
  function api(
    path: string,
    init: RequestInit & { accessToken?: string; workspace?: string } = {},
  ): Promise<Response> {
    const { accessToken, workspace, headers: extra, ...rest } = init;
    const headers = new Headers(extra);
    if (!headers.has("accept")) headers.set("accept", "application/json");
    if (accessToken) headers.set("authorization", `Bearer ${accessToken}`);
    if (workspace && !headers.has(workspaceHeader)) headers.set(workspaceHeader, workspace);
    return upstreamFetch(`${apiUrl}${path}`, { cache: "no-store", ...rest, headers });
  }

  /** The signed-in user (`GET /api/auth/me`), or `null` without a token or when Navree refuses it. */
  async function me<User = unknown>(accessToken: string | undefined, workspace?: string): Promise<User | null> {
    if (!accessToken) return null;
    const response = await api(mePath, { accessToken, workspace });
    if (response.status === 401 || response.status === 403) {
      await discard(response);
      return null;
    }
    if (!response.ok) throw new Error(`GET ${mePath} answered ${response.status}`);
    return (await response.json()) as User;
  }

  return {
    proxy,
    login,
    verifyTwoFactor,
    logout,
    refreshSession,
    handle,
    ensureSession,
    api,
    me,
    originAllowed,
    workspaceOf,
    /** The cookie names this BFF uses. */
    cookieNames: names as Required<CookieNames>,
  };
}

export type Bff = ReturnType<typeof createBff>;
