/**
 * The server side of a Next.js app on a Navree API: the backend-for-frontend
 * route handlers, `auth()` and `client()` for server components, and the
 * proxy (Next.js middleware) that keeps pages behind sign-in and refreshes
 * expired tokens before a page renders.
 */
import { bffOptionsFromEnv, createBff, type Bff, type BffOptions } from "@navree/bff";
import { createNavreeClient, type NavreeClient } from "@navree/client";
import { cookies } from "next/headers";
import { NextResponse, type NextRequest } from "next/server";
import { cache } from "react";

export interface NavreeNextOptions extends Partial<BffOptions> {
  /** Where settings are read (default `process.env`, at the first request). */
  env?: Record<string, string | undefined>;
}

/** What `auth()` returns for a signed-in request. */
export interface NavreeSession<User> {
  user: User;
  /** The access token, for your own server-side calls to the API. */
  accessToken: string;
  /** The workspace from the workspace cookie, when one is chosen. */
  workspace?: string;
}

export interface NavreeProxyOptions {
  /** The sign-in page (default: `pages.signIn` of the BFF settings, else `/login`); always public. */
  loginPath?: string;
  /** Pages anyone may open: exact paths, or prefixes ending in `/*` (`/docs/*`). */
  publicPaths?: string[];
  /**
   * Path prefixes the proxy never touches (default: `/api/`, `/broadcasting/`, `/_next/`, and the
   * social sign-in routes, `/auth/social/` and `/auth/callback/` unless `socialPaths` moves them).
   */
  passThrough?: string[];
  /** Send a signed-in browser that opens the sign-in page here (off by default). */
  signedInRedirect?: string;
}

type RouteHandler = (request: Request) => Promise<Response>;

const DEFAULT_PASS_THROUGH = ["/api/", "/broadcasting/", "/_next/", "/__nextjs"];

function matches(pathname: string, patterns: readonly string[]): boolean {
  return patterns.some((pattern) =>
    pattern.endsWith("/*") ? pathname === pattern.slice(0, -2) || pathname.startsWith(pattern.slice(0, -1)) : pathname === pattern,
  );
}

/** A page navigation (a document or a React Server Components request), as opposed to an API call or an asset. */
function isNavigation(request: NextRequest): boolean {
  const method = request.method.toUpperCase();
  if (method !== "GET" && method !== "HEAD") return false;
  if (request.headers.get("rsc") !== null) return true;
  return (request.headers.get("accept") ?? "").includes("text/html");
}

/**
 * Navree for one Next.js app. Create it once (`lib/navree.ts`) and use it in
 * route handlers, server components and `proxy.ts`:
 *
 * ```ts
 * export const navree = createNavree();
 * // app/api/[...path]/route.ts
 * export const { GET, POST, PUT, PATCH, DELETE, HEAD } = navree.handlers;
 * ```
 */
export function createNavree(options: NavreeNextOptions = {}) {
  const { env, ...overrides } = options;
  let resolved: BffOptions | undefined;
  let instance: Bff | undefined;

  /** The settings: `NAVREE_*` variables, then the options given here. Read once, at the first request. */
  function settings(): BffOptions {
    if (!resolved) {
      const fromEnv = bffOptionsFromEnv(env ?? process.env);
      resolved = { ...fromEnv, ...stripUndefined(overrides) } as BffOptions;
    }
    return resolved;
  }

  /** The BFF, created on first use so one refresh map serves every request of this server. */
  function bff(): Bff {
    instance ??= createBff(settings());
    return instance;
  }

  const handle: RouteHandler = (request) => bff().handle(request);

  /** Route handlers for a catch-all route (`app/api/[...path]/route.ts`, `app/broadcasting/auth/route.ts`). */
  const handlers = {
    GET: handle,
    HEAD: handle,
    POST: handle,
    PUT: handle,
    PATCH: handle,
    DELETE: handle,
    OPTIONS: handle,
  };

  async function sessionCookies(): Promise<{ access?: string; workspace?: string }> {
    const store = await cookies();
    const names = bff().cookieNames;
    return { access: store.get(names.access)?.value, workspace: store.get(names.workspace)?.value };
  }

  /**
   * The signed-in user in a server component, route handler or server action,
   * or `null`. Cached per request. The proxy has already refreshed an expired
   * token, so this never refreshes (a server component can't set cookies).
   */
  const auth = cache(async <User = unknown>(): Promise<NavreeSession<User> | null> => {
    const { access, workspace } = await sessionCookies();
    if (!access) return null;
    const user = await bff().me<User>(access, workspace);
    if (!user) return null;
    return { user, accessToken: access, ...(workspace ? { workspace } : {}) };
  }) as <User = unknown>() => Promise<NavreeSession<User> | null>;

  /** A typed client that calls the API from the server with the session's token and workspace. */
  async function client<Paths extends object>(): Promise<NavreeClient<Paths>> {
    const { access, workspace } = await sessionCookies();
    const { apiUrl, workspaceHeader, fetch: upstreamFetch } = settings();
    return createNavreeClient<Paths>({
      baseUrl: apiUrl,
      token: () => access ?? null,
      workspace,
      workspaceHeader,
      fetch: upstreamFetch ? (request) => upstreamFetch(request) : undefined,
    });
  }

  /**
   * The proxy (`proxy.ts`, or `middleware.ts` before Next.js 16): pages
   * behind sign-in, and an expired access token refreshed before the page
   * renders, so `auth()` and the browser both get the new tokens.
   */
  function proxy(proxyOptions: NavreeProxyOptions = {}) {
    return async function navreeProxy(request: NextRequest): Promise<NextResponse> {
      // The BFF is made here, at the first request, not when proxy.ts loads.
      const { pages, socialPaths } = bff();
      const loginPath = proxyOptions.loginPath ?? pages.signIn;
      const publicPaths = [loginPath, ...(proxyOptions.publicPaths ?? [])];
      const passThrough = proxyOptions.passThrough ?? [
        ...DEFAULT_PASS_THROUGH,
        `${socialPaths.start}/`,
        `${socialPaths.callback}/`,
      ];

      const { pathname, search } = request.nextUrl;
      if (passThrough.some((prefix) => pathname.startsWith(prefix)) || /\.[a-z0-9]+$/i.test(pathname)) {
        return NextResponse.next();
      }
      if (!isNavigation(request)) return NextResponse.next();

      const isPublic = matches(pathname, publicPaths);
      const session = await bff().ensureSession(request);
      const signedIn = session.status === "active" || session.status === "refreshed";

      let response: NextResponse;
      if (signedIn && proxyOptions.signedInRedirect && pathname === loginPath) {
        response = NextResponse.redirect(new URL(proxyOptions.signedInRedirect, request.url));
      } else if (session.status === "unavailable") {
        // The API could not be reached to refresh the token, which is not a sign-out. The cookies
        // stay and the page is served; what it shows when its own calls fail is up to it.
        response = NextResponse.next();
      } else if (!signedIn && !isPublic) {
        const target = new URL(loginPath, request.url);
        target.searchParams.set("next", `${pathname}${search}`);
        response = NextResponse.redirect(target);
      } else if (session.status === "refreshed" && session.cookieHeader) {
        const headers = new Headers(request.headers);
        headers.set("cookie", session.cookieHeader);
        response = NextResponse.next({ request: { headers } });
      } else {
        response = NextResponse.next();
      }
      for (const value of session.setCookie) response.headers.append("set-cookie", value);
      return response;
    };
  }

  return { handlers, auth, client, proxy, bff };
}

function stripUndefined<T extends object>(value: T): Partial<T> {
  return Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined)) as Partial<T>;
}

export type Navree = ReturnType<typeof createNavree>;
