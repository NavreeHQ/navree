/**
 * The typed client: the typed fetch core (`fetch.ts`) on the `paths` type that
 * `navree openapi:client` generates from the app's OpenAPI document, with Navree's
 * conventions: cookie or bearer mode, workspace and request id headers, the error shape.
 */
import { NavreeError } from "./errors.js";
import { createClient, type Client } from "./fetch.js";
import { listQuerySerializer } from "./list-query.js";

type MaybePromise<T> = T | Promise<T>;

export interface NavreeClientOptions {
  /**
   * Where the API (or the backend-for-frontend) is. Default `""`: the
   * page's own origin, which is what a browser behind a BFF wants.
   */
  baseUrl?: string;
  /**
   * Bearer mode: the access token, or a function returning it before every
   * request (`null` sends none). Without a token the client is in cookie
   * mode: the BFF holds the tokens and the browser sends only its cookies.
   */
  token?: string | (() => MaybePromise<string | null | undefined>);
  /** Fetch credentials. Default `same-origin` in cookie mode, `omit` in bearer mode. */
  credentials?: RequestCredentials;
  /** The current workspace (a slug or an id), sent as {@link workspaceHeader}. */
  workspace?: string | (() => string | null | undefined);
  /** The workspace header, as `workspace.header` configures it on the API. Default `X-Workspace`. */
  workspaceHeader?: string;
  /**
   * `X-Request-Id` for every request, so a failure can be found in the API's
   * logs: `true` (default) makes a UUID, a function makes your own, `false` sends none.
   */
  requestId?: boolean | (() => string);
  /** Headers sent with every request (a request's own headers win). */
  headers?: Record<string, string>;
  /** The fetch to use (tests, server runtimes). Default: `globalThis.fetch`, looked up per request. */
  fetch?: (request: Request) => Promise<Response>;
  /** Called on every 401: the session is over (redirect to sign-in). */
  onUnauthorized?: (response: Response, request: Request) => void;
  /** Called on every 403 with the parsed error (a toast, a notice). */
  onForbidden?: (error: NavreeError, request: Request) => void;
}

/** A UUID for `X-Request-Id`: `crypto.randomUUID` where it exists (secure contexts), random bytes otherwise. */
export function newRequestId(): string {
  const c = globalThis.crypto;
  if (typeof c?.randomUUID === "function") return c.randomUUID();
  const bytes = new Uint8Array(16);
  if (typeof c?.getRandomValues === "function") c.getRandomValues(bytes);
  else for (let i = 0; i < 16; i++) bytes[i] = Math.floor(Math.random() * 256);
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** The client: `GET`, `POST`, ... on the typed fetch core, plus `raw` for responses that aren't JSON. */
export type NavreeClient<Paths extends object> = Client<Paths> & {
  /**
   * A plain request with the client's headers and hooks: CSV exports, event
   * streams, `/broadcasting/auth`. `path` is relative to `baseUrl`.
   */
  raw(path: string, init?: RequestInit): Promise<Response>;
};

/**
 * A typed client for a Navree API.
 *
 * ```ts
 * import { createNavreeClient, unwrap } from "@navree/client";
 * import type { paths } from "./schema.js";
 *
 * export const api = createNavreeClient<paths>({ workspace: () => currentWorkspace() });
 * const deal = await unwrap(api.GET("/api/v1/deals/{id}", { params: { path: { id } } }));
 * ```
 */
export function createNavreeClient<Paths extends object>(options: NavreeClientOptions = {}): NavreeClient<Paths> {
  const baseUrl = (options.baseUrl ?? "").replace(/\/+$/, "");
  const bearer = options.token !== undefined;
  const credentials = options.credentials ?? (bearer ? "omit" : "same-origin");
  const workspaceHeader = options.workspaceHeader ?? "X-Workspace";
  const send = options.fetch ?? ((request: Request) => globalThis.fetch(request));

  async function prepare(request: Request): Promise<Request> {
    const headers = request.headers;
    if (!headers.has("accept")) headers.set("accept", "application/json");
    for (const [name, value] of Object.entries(options.headers ?? {})) {
      if (!headers.has(name)) headers.set(name, value);
    }
    if (options.requestId !== false && !headers.has("x-request-id")) {
      headers.set("x-request-id", typeof options.requestId === "function" ? options.requestId() : newRequestId());
    }
    if (options.workspace !== undefined && !headers.has(workspaceHeader)) {
      const workspace = typeof options.workspace === "function" ? options.workspace() : options.workspace;
      if (workspace) headers.set(workspaceHeader, workspace);
    }
    if (bearer && !headers.has("authorization")) {
      const token = typeof options.token === "function" ? await options.token() : options.token;
      if (token) headers.set("authorization", `Bearer ${token}`);
    }
    return request;
  }

  async function fetchWithHooks(input: Request): Promise<Response> {
    const request = await prepare(input);
    const response = await send(request);
    if (response.status === 401) {
      options.onUnauthorized?.(response, request);
    } else if (response.status === 403 && options.onForbidden) {
      const body: unknown = await response
        .clone()
        .json()
        .catch(() => null);
      options.onForbidden(NavreeError.fromResponse(response, body), request);
    }
    return response;
  }

  const client = createClient<Paths>({
    baseUrl,
    credentials,
    fetch: fetchWithHooks,
    querySerializer: listQuerySerializer,
  });

  return Object.assign(client, {
    raw: (path: string, init?: RequestInit) =>
      fetchWithHooks(new Request(`${baseUrl}${path}`, { credentials, ...init })),
  });
}

type FetchResult<T> = { data?: T; error?: unknown; response: Response };

/**
 * The data of a typed fetch result, or a thrown {@link NavreeError}
 * (with `network_error` when the request got no answer).
 */
export async function unwrap<T>(result: Promise<FetchResult<T>>): Promise<T> {
  let settled: FetchResult<T>;
  try {
    settled = await result;
  } catch (cause) {
    if (cause instanceof NavreeError) throw cause;
    throw NavreeError.network(cause);
  }
  const { data, error, response } = settled;
  if (!response.ok || error !== undefined) throw NavreeError.fromResponse(response, error);
  return data as T;
}

type WithQueryOperation<Operation, Query> = Operation extends { parameters: infer P }
  ? Omit<Operation, "parameters"> & { parameters: Omit<P, "query"> & { query?: Query } }
  : Operation;

/**
 * `Paths` with the query parameters of some `GET` routes replaced, for list
 * endpoints whose OpenAPI document doesn't describe their list query:
 *
 * ```ts
 * type Paths = WithQuery<paths, { "/api/v1/deals": ListQuery }>;
 * ```
 */
export type WithQuery<Paths, Queries extends { [P in keyof Paths]?: object }> = {
  [P in keyof Paths]: P extends keyof Queries
    ? Paths[P] extends { get: infer Get }
      ? Omit<Paths[P], "get"> & { get: WithQueryOperation<Get, Queries[P]> }
      : Paths[P]
    : Paths[P];
};
