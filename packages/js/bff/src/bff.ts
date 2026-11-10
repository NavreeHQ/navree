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
 * - `socialRedirect`, `socialCallback` and `verifyEmail` are the three browser-facing
 *   GET routes of social sign-in and the link in the verification mail: each ends in a
 *   redirect to a page of the app, with the tokens in the cookies like a password sign-in;
 * - a refresh the API refuses ends the session (the cookies are cleared); a refresh that
 *   could not finish because the API was unreachable does not (the cookies stay);
 * - every non-GET request must come from the app's own origin (403 otherwise),
 *   which is the CSRF defence for the cookie hop;
 * - the browser's `X-Forwarded-For` / `X-Real-IP` never reach Navree; the BFF
 *   sends its own `X-Forwarded-For` only when it knows the client address
 *   (see `trustedProxyHops` and `clientIpHeader`).
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
/** The social sign-in state, between the redirect to the provider and the callback. */
export const SOCIAL_COOKIE = "navree_social";

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
  social?: string;
}

/** The pages of the app that the BFF's redirects end on. */
export interface BffPages {
  /** The sign-in page (default `/login`). A failed social sign-in lands here with `?error=`. */
  signIn?: string;
  /** The page that asks for the two-factor code (default `/two-factor`). */
  twoFactor?: string;
  /** The page the verification link ends on, with `?status=verified|invalid|unreachable` (default `/verify-email`). */
  verifyEmail?: string;
}

/** Where the app mounts the two social sign-in routes. */
export interface BffSocialPaths {
  /** `GET {start}/{provider}?next=` starts a sign-in (default `/auth/social`). */
  start?: string;
  /** `GET {callback}/{provider}?code&state` is where the provider sends the browser back (default `/auth/callback`). */
  callback?: string;
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
  /**
   * A header that the platform or the one proxy in front of this server sets to the bare client
   * address, replacing any value the browser sent (e.g. `x-real-ip` behind a proxy configured to
   * overwrite it, `cf-connecting-ip`, `fly-client-ip`). The BFF believes it before it looks at
   * `X-Forwarded-For`, so it works with a single trusted hop and no `X-Forwarded-For` chain.
   * Set it only when the server is reachable through that proxy alone.
   */
  clientIpHeader?: string;
  /** The API's auth routes (default `/api/auth`): login, refresh, logout, `2fa/verify`. */
  authPrefix?: string;
  /** The API's current-user route (default `/api/auth/me`). */
  mePath?: string;
  /** The workspace header, as the API's `workspace.header` names it (default `X-Workspace`). */
  workspaceHeader?: string;
  /** Cookie names, for several apps on one domain (defaults `navree_at`, `navree_rt`, `navree_2fa`, `navree_workspace`, `navree_social`). */
  cookieNames?: CookieNames;
  /** The pages of the app that social sign-in, the verification link and `redirectNavigations` end on. */
  pages?: BffPages;
  /** Where the app mounts social sign-in (defaults `/auth/social` and `/auth/callback`). */
  socialPaths?: BffSocialPaths;
  /**
   * Send a page request that the API refuses with 401 (the session is over) to `pages.signIn` with
   * `?next=`, instead of passing the 401 on. For a server-rendered tool mounted through the BFF, such
   * as the admin panel at `/admin`. Off by default; a call that doesn't ask for HTML gets the 401.
   */
  redirectNavigations?: boolean;
  /** Maps a request path to the API path `handle` forwards to (default: the same path). */
  upstreamPath?: (pathname: string) => string;
  /** Refresh an access token this many seconds before it expires (`ensureSession`; default 30). */
  refreshSkewSeconds?: number;
}

/** What `ensureSession` found. */
export interface Session {
  /**
   * `anonymous`: no token cookies; `active`: the access token is usable;
   * `refreshed`: it had expired and was refreshed; `expired`: the API refused the refresh, so the
   * session is over; `unavailable`: the access token had expired and the API could not be reached
   * to refresh it, so nothing is known yet. The cookies are kept: serve the page and let it show
   * its own error, or try again.
   */
  status: "anonymous" | "active" | "refreshed" | "expired" | "unavailable";
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
  const clientIpHeader = options.clientIpHeader?.trim().toLowerCase() || null;
  const authPrefix = (options.authPrefix ?? "/api/auth").replace(/\/+$/, "");
  const challengePath = `${authPrefix}/2fa`;
  const mePath = options.mePath ?? `${authPrefix}/me`;
  const workspaceHeader = (options.workspaceHeader ?? "X-Workspace").toLowerCase();
  const names = {
    access: options.cookieNames?.access ?? ACCESS_COOKIE,
    refresh: options.cookieNames?.refresh ?? REFRESH_COOKIE,
    challenge: options.cookieNames?.challenge ?? CHALLENGE_COOKIE,
    workspace: options.cookieNames?.workspace ?? WORKSPACE_COOKIE,
    social: options.cookieNames?.social ?? SOCIAL_COOKIE,
  };
  const pages = {
    signIn: options.pages?.signIn ?? "/login",
    twoFactor: options.pages?.twoFactor ?? "/two-factor",
    verifyEmail: options.pages?.verifyEmail ?? "/verify-email",
  };
  const socialStart = (options.socialPaths?.start ?? "/auth/social").replace(/\/+$/, "");
  const socialCallbackPath = (options.socialPaths?.callback ?? "/auth/callback").replace(/\/+$/, "");
  const upstreamPathOf = options.upstreamPath ?? ((pathname: string) => pathname);
  const refreshSkew = options.refreshSkewSeconds ?? 30;

  /**
   * What a refresh came to: new tokens; `rejected` (the API said no: the token expired, was revoked
   * or was used twice, so the session is over); or `unavailable` (the API could not be reached or
   * failed, so nothing is known and the cookies must stay).
   */
  type Refresh = { kind: "ok"; pair: TokenPair } | { kind: "rejected" } | { kind: "unavailable" };

  /**
   * Refresh tokens rotate and reuse revokes the whole family, so concurrent
   * requests holding the same refresh cookie must share one refresh call, and
   * requests that arrive shortly after with the old cookie get the new pair.
   */
  const refreshes = new Map<string, { promise: Promise<Refresh>; settledAt?: number }>();

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

  async function callNavreeRefresh(refreshToken: string, request: Request): Promise<Refresh> {
    try {
      const response = await upstreamFetch(`${apiUrl}${authPrefix}/refresh`, {
        method: "POST",
        cache: "no-store",
        headers: { "content-type": "application/json", accept: "application/json", ...forwardedFor(request) },
        body: JSON.stringify({ refresh_token: refreshToken }),
      });
      if (!response.ok) {
        await discard(response);
        // A 4xx other than 408 and 429 is the API's answer about this token. Anything else
        // (an overloaded or restarting API, a rate limit) says nothing about the session.
        const answered = response.status >= 400 && response.status < 500 && response.status !== 408 && response.status !== 429;
        return answered ? { kind: "rejected" } : { kind: "unavailable" };
      }
      const pair = (await response.json()) as TokenPair;
      return pair.access_token && pair.refresh_token ? { kind: "ok", pair } : { kind: "unavailable" };
    } catch {
      return { kind: "unavailable" };
    }
  }

  /** Single-flight refresh keyed by the refresh token being spent. */
  function refresh(refreshToken: string, request: Request): Promise<Refresh> {
    const now = Date.now();
    for (const [key, entry] of refreshes) {
      if (entry.settledAt !== undefined && now - entry.settledAt > refreshGraceMs) refreshes.delete(key);
    }
    const existing = refreshes.get(refreshToken);
    if (existing) return existing.promise;
    const entry: { promise: Promise<Refresh>; settledAt?: number } = {
      promise: callNavreeRefresh(refreshToken, request).then((outcome) => {
        entry.settledAt = Date.now();
        if (outcome.kind !== "ok") refreshes.delete(refreshToken);
        return outcome;
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
      if (value) headers.set(name, name === "location" ? ownOrigin(value) : value);
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
   * A redirect the API sent to its own address (`Location: http://api:8080/admin/users`) becomes a
   * path on this app's origin, so the browser stays here. Any other location is left as it is.
   */
  function ownOrigin(location: string): string {
    if (location === apiUrl) return "/";
    return location.startsWith(`${apiUrl}/`) ? location.slice(apiUrl.length) : location;
  }

  /** A `303 See Other` to a page of this app, with `headers` (cookies). */
  function redirect(location: string, headers = new Headers()): Response {
    headers.set("location", location);
    return new Response(null, { status: 303, headers });
  }

  /** A GET for a page (the browser asks for HTML), as opposed to a call from the app's own scripts. */
  function isNavigation(request: Request): boolean {
    const method = request.method.toUpperCase();
    if (method !== "GET" && method !== "HEAD") return false;
    return (request.headers.get("accept") ?? "").includes("text/html");
  }

  /** `next` when it is a path on this origin, else `/`: never an open redirect. */
  function localPath(next: string | null | undefined): string {
    return next && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/\\") ? next : "/";
  }

  /** The sign-in page, with the page the browser asked for in `next`. */
  function signInUrl(request: Request): string {
    const url = new URL(request.url);
    return `${pages.signIn}?next=${encodeURIComponent(`${url.pathname}${url.search}`)}`;
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
      const outcome = await refresh(tokens.refresh, request);
      if (outcome.kind === "unavailable") return unreachable();
      refreshed = outcome.kind === "ok" ? outcome.pair : null;
      refreshFailed = outcome.kind === "rejected";
    }

    let upstream: Response;
    try {
      upstream = await send(refreshed?.access_token ?? tokens.access);
      if (upstream.status === 401 && !refreshed && tokens.refresh && !refreshFailed) {
        const outcome = await refresh(tokens.refresh, request);
        if (outcome.kind === "ok") {
          refreshed = outcome.pair;
          await discard(upstream);
          upstream = await send(refreshed.access_token);
        } else if (outcome.kind === "rejected") {
          refreshFailed = true;
        } else {
          // The API couldn't be asked: the 401 says nothing about the session, so the cookies stay.
          await discard(upstream);
          return unreachable();
        }
      }
    } catch {
      return unreachable();
    }

    // New pair -> set both cookies. The API refused the refresh, or refused the token with nothing to
    // refresh it with -> the session is over: clear both so the browser goes back to sign-in.
    const cookies = new Headers();
    const sessionOver = !refreshed && (refreshFailed || (upstream.status === 401 && Boolean(tokens.access)));
    if (refreshed) setTokenCookies(cookies, refreshed);
    else if (sessionOver) clearTokenCookies(cookies);
    if (upstream.status === 401 && options.redirectNavigations && isNavigation(request)) {
      await discard(upstream);
      return redirect(signInUrl(request), cookies);
    }
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
          const outcome = await refresh(tokens.refresh, request);
          response = outcome.kind === "ok" ? await revoke(outcome.pair.access_token, outcome.pair.refresh_token) : null;
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
    const outcome: Refresh = tokens.refresh ? await refresh(tokens.refresh, request) : { kind: "rejected" };
    // The API could not be reached: nothing is known about the session, so its cookies stay.
    if (outcome.kind === "unavailable") return unreachable();
    const headers = new Headers();
    if (outcome.kind === "rejected") {
      clearTokenCookies(headers);
      return errorResponse(401, "Your session has expired", "session_expired", headers);
    }
    setTokenCookies(headers, outcome.pair);
    return new Response(null, { status: 204, headers });
  }

  /** The provider name in `{base}/{provider}`, or `undefined` when `pathname` is not one of those. */
  function providerOf(pathname: string, base: string): string | undefined {
    if (!pathname.startsWith(`${base}/`)) return undefined;
    const rest = pathname.slice(base.length + 1);
    return /^[A-Za-z0-9_-]{1,64}$/.test(rest) ? rest : undefined;
  }

  /** Whether `path` is the API's `{authPrefix}/email/verify/{id}/{hash}`. */
  function isVerifyPath(path: string): boolean {
    const prefix = `${authPrefix}/email/verify/`;
    if (!path.startsWith(prefix)) return false;
    const parts = path.slice(prefix.length).split("/");
    return parts.length === 2 && parts.every((part) => part.length > 0);
  }

  function isWebUrl(value: string): boolean {
    try {
      const { protocol } = new URL(value);
      return protocol === "https:" || protocol === "http:";
    } catch {
      return false;
    }
  }

  /**
   * `GET {socialPaths.start}/{provider}?next=/w/agra`: starts a social sign-in. The API answers with
   * the provider's URL and a sealed state; the state waits in an httpOnly cookie that only the
   * callback route receives, and the browser goes to the provider. Failures end on `pages.signIn`
   * with `?error=social_unavailable` or `api_unreachable`.
   */
  async function socialRedirect(request: Request, provider: string): Promise<Response> {
    const next = localPath(new URL(request.url).searchParams.get("next"));
    let upstream: Response;
    try {
      upstream = await api(
        `${authPrefix}/social/${encodeURIComponent(provider)}/redirect?return_to=${encodeURIComponent(next)}`,
        { request },
      );
    } catch {
      return redirect(`${pages.signIn}?error=api_unreachable`);
    }
    if (!upstream.ok) {
      await discard(upstream);
      return redirect(`${pages.signIn}?error=social_unavailable`);
    }
    const { url, state } = (await upstream.json().catch(() => ({}))) as { url?: string; state?: string };
    if (!url || !state || !isWebUrl(url)) return redirect(`${pages.signIn}?error=social_unavailable`);
    const headers = new Headers();
    headers.append("set-cookie", cookie(names.social, state, { maxAge: 600, path: socialCallbackPath }));
    return redirect(url, headers);
  }

  /**
   * `GET {socialPaths.callback}/{provider}?code&state`: the provider sends the browser back here. The
   * state must match the cookie. The API exchanges the code and answers like a password sign-in: the
   * tokens go into the cookies and the browser lands on the page the sign-in started from, or a
   * two-factor challenge goes into its cookie and `pages.twoFactor` finishes it. Failures end on
   * `pages.signIn` with `?error=social_state`, `social_failed` or `api_unreachable`.
   */
  async function socialCallback(request: Request, provider: string): Promise<Response> {
    const params = new URL(request.url).searchParams;
    const code = params.get("code");
    const state = params.get("state");
    const expected = readCookie(request, names.social);
    // The state is spent whatever comes of it.
    const headers = new Headers();
    headers.append("set-cookie", cookie(names.social, "", { maxAge: 0, path: socialCallbackPath }));
    if (!code || !state || !expected || state !== expected) {
      return redirect(`${pages.signIn}?error=social_state`, headers);
    }

    let upstream: Response;
    try {
      upstream = await api(`${authPrefix}/social/${encodeURIComponent(provider)}/callback`, {
        method: "POST",
        request,
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ code, state }),
      });
    } catch {
      return redirect(`${pages.signIn}?error=api_unreachable`, headers);
    }
    if (!upstream.ok) {
      await discard(upstream);
      return redirect(`${pages.signIn}?error=social_failed`, headers);
    }
    const data = (await upstream.json().catch(() => ({}))) as LoginBody & { return_to?: string };
    const next = localPath(data.return_to);
    if (data.two_factor_required === true && typeof data.challenge_token === "string") {
      setChallengeCookie(headers, data.challenge_token, data.expires_in);
      return redirect(`${pages.twoFactor}?next=${encodeURIComponent(next)}`, headers);
    }
    if (!isTokenPair(data)) return redirect(`${pages.signIn}?error=social_failed`, headers);
    setTokenCookies(headers, data);
    return redirect(next, headers);
  }

  /**
   * `GET {authPrefix}/email/verify/{id}/{hash}?expires&signature`: the link in the verification mail.
   * The API checks the signature and marks the address verified; the browser lands on
   * `pages.verifyEmail` with `?status=verified`, `invalid` or `unreachable`.
   */
  async function verifyEmail(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const path = upstreamPathOf(url.pathname.replace(/\/+$/, ""));
    if (!isVerifyPath(path)) return errorResponse(404, "Not Found", "not_found");
    try {
      const upstream = await api(`${path}${url.search}`, { request });
      await discard(upstream);
      const status = upstream.ok ? "verified" : upstream.status >= 500 ? "unreachable" : "invalid";
      return redirect(`${pages.verifyEmail}?status=${status}`);
    } catch {
      return redirect(`${pages.verifyEmail}?status=unreachable`);
    }
  }

  /**
   * One entry point for a catch-all route: `POST {authPrefix}/login`, `/2fa/verify`,
   * `/logout` and `/refresh` go to their handlers; the GET routes of social sign-in and of the
   * verification link go to theirs; everything else is proxied to the same path (or
   * `upstreamPath(pathname)`).
   */
  function handle(request: Request): Promise<Response> {
    const pathname = new URL(request.url).pathname.replace(/\/+$/, "") || "/";
    const upstream = upstreamPathOf(pathname);
    if (request.method.toUpperCase() === "GET") {
      const startProvider = providerOf(pathname, socialStart);
      if (startProvider) return socialRedirect(request, startProvider);
      const callbackProvider = providerOf(pathname, socialCallbackPath);
      if (callbackProvider) return socialCallback(request, callbackProvider);
      if (isVerifyPath(upstream)) return verifyEmail(request);
    }
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
    const outcome = await refresh(tokens.refresh as string, request);
    if (outcome.kind === "rejected") return { status: "expired", setCookie: clearedTokenCookies() };
    if (outcome.kind === "unavailable") {
      // The API could not be asked. A token that is only inside the refresh margin still works.
      const exp = tokens.access ? tokenExpiresAt(tokens.access) : undefined;
      if (tokens.access && exp !== undefined && exp > Date.now() / 1000) {
        return { status: "active", accessToken: tokens.access, setCookie: [] };
      }
      return { status: "unavailable", setCookie: [] };
    }
    const pair = outcome.pair;
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
    init: RequestInit & { accessToken?: string; workspace?: string; request?: Request } = {},
  ): Promise<Response> {
    const { accessToken, workspace, request, headers: extra, ...rest } = init;
    const headers = new Headers(extra);
    if (!headers.has("accept")) headers.set("accept", "application/json");
    if (accessToken) headers.set("authorization", `Bearer ${accessToken}`);
    if (workspace && !headers.has(workspaceHeader)) headers.set(workspaceHeader, workspace);
    // The browser the call is for: the API sees the address the BFF vouches for, not the BFF's own.
    if (request) {
      for (const [name, value] of Object.entries(forwardedFor(request))) headers.set(name, value);
    }
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
    socialRedirect,
    socialCallback,
    verifyEmail,
    handle,
    ensureSession,
    api,
    me,
    originAllowed,
    workspaceOf,
    /** The cookie names this BFF uses. */
    cookieNames: names as Required<CookieNames>,
    /** The app's pages its redirects end on. */
    pages: pages as Required<BffPages>,
    /** Where the app mounts social sign-in. */
    socialPaths: { start: socialStart, callback: socialCallbackPath } as Required<BffSocialPaths>,
  };
}

export type Bff = ReturnType<typeof createBff>;
