/**
 * The backend-for-frontend as h3 handlers, for Nitro (Nuxt's server). The
 * module registers them; they are exported for apps that mount routes
 * themselves and for tests.
 */
import { createBff, type Bff, type BffOptions } from "@navree/bff";
import {
  appendResponseHeader,
  defineEventHandler,
  getRequestHeader,
  getRequestURL,
  sendRedirect,
  toWebRequest,
  type EventHandler,
  type H3Event,
} from "h3";

/** Gives the handlers their BFF for a request. */
export type BffSource = (event?: H3Event) => Bff;

/**
 * One BFF per settings: created on first use, so settings are read at
 * runtime, and shared by every request of this server, so concurrent
 * requests share one token refresh.
 */
export function bffResolver(settings: (event?: H3Event) => BffOptions): BffSource {
  let current: { key: string; bff: Bff } | undefined;
  return (event) => {
    const options = settings(event);
    const key = JSON.stringify({ ...options, fetch: undefined });
    if (!current || current.key !== key) current = { key, bff: createBff(options) };
    return current.bff;
  };
}

/** The BFF's catch-all: sign-in, two-factor, sign-out and refresh, and the proxy for everything else. */
export function createBffHandler(source: BffSource): EventHandler {
  return defineEventHandler((event) => source(event).handle(toWebRequest(event)));
}

export interface SessionGateOptions {
  /** The sign-in page (default `/login`); always public. */
  loginPath?: string;
  /** Pages anyone may open: exact paths, or prefixes ending in `/*`. */
  publicPaths?: string[];
  /**
   * Path prefixes the gate never touches (default: `/api/`, `/broadcasting/`, `/_nuxt/`, `/__nuxt`,
   * `/_ipx/`, and the social sign-in routes, `/auth/social/` and `/auth/callback/` unless
   * `socialPaths` moves them).
   */
  passThrough?: string[];
}

const DEFAULT_PASS_THROUGH = ["/api/", "/broadcasting/", "/_nuxt/", "/__nuxt", "/_ipx/"];

function matches(pathname: string, patterns: readonly string[]): boolean {
  return patterns.some((pattern) =>
    pattern.endsWith("/*") ? pathname === pattern.slice(0, -2) || pathname.startsWith(pattern.slice(0, -1)) : pathname === pattern,
  );
}

/**
 * Server middleware for pages behind sign-in: a page request without a
 * session goes to `loginPath?next=…`; an expired access token is refreshed
 * first, for the browser (`Set-Cookie`) and for the rest of this request
 * (the rewritten `Cookie` header that server-side rendering reads).
 */
export function createSessionGate(source: BffSource, options: SessionGateOptions = {}): EventHandler {
  const loginPath = options.loginPath ?? "/login";
  const publicPaths = [loginPath, ...(options.publicPaths ?? [])];

  return defineEventHandler(async (event) => {
    if (event.method !== "GET" && event.method !== "HEAD") return;
    const url = getRequestURL(event);
    const { pathname } = url;
    const bff = source(event);
    const passThrough = options.passThrough ?? [
      ...DEFAULT_PASS_THROUGH,
      `${bff.socialPaths.start}/`,
      `${bff.socialPaths.callback}/`,
    ];
    if (passThrough.some((prefix) => pathname.startsWith(prefix)) || /\.[a-z0-9]+$/i.test(pathname)) return;
    if (!(getRequestHeader(event, "accept") ?? "").includes("text/html")) return;

    const session = await bff.ensureSession(toWebRequest(event));
    for (const value of session.setCookie) appendResponseHeader(event, "set-cookie", value);
    if (session.status === "refreshed" && session.cookieHeader) {
      event.node.req.headers.cookie = session.cookieHeader;
    }
    // The API could not be reached to refresh the token, which is not a sign-out: the cookies stay
    // (nothing was cleared above) and the page is served.
    if (session.status === "unavailable") return;
    const signedIn = session.status === "active" || session.status === "refreshed";
    if (!signedIn && !matches(pathname, publicPaths)) {
      const target = `${pathname}${url.search}`;
      return sendRedirect(event, `${loginPath}?next=${encodeURIComponent(target)}`, 302);
    }
  });
}

/** The signed-in user of a request, for server routes and server-side rendering. */
export async function getNavreeSession<User = unknown>(
  event: H3Event,
  bff: Bff,
): Promise<{ user: User; accessToken: string; workspace?: string } | null> {
  const request = toWebRequest(event);
  const session = await bff.ensureSession(request);
  if (!session.accessToken) return null;
  const workspace = bff.workspaceOf(request);
  const user = await bff.me<User>(session.accessToken, workspace);
  if (!user) return null;
  return { user, accessToken: session.accessToken, ...(workspace ? { workspace } : {}) };
}
