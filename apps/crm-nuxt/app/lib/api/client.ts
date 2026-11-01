import createClient from "./fetch";
import { ApiError } from "./errors";
import type { paths } from "./types";

export interface ApiClientOptions {
  /** Origin of the BFF. Empty string (default) = this app (same-origin route handlers). */
  baseUrl?: string;
  /** Underlying fetch. Defaults to `globalThis.fetch` (resolved lazily so MSW/test spies see it). */
  fetch?: (request: Request) => Promise<Response>;
  /** Called on any 401: the BFF couldn't refresh the session. */
  onUnauthorized?: (response: Response, request: Request) => void;
  /** Called on any 403 (after the body is parsed into an ApiError). */
  onForbidden?: (error: ApiError, request: Request) => void;
}

/**
 * Builds the typed client the browser uses. It only ever calls the app's own
 * `/api/*` route handlers (the BFF), which hold the JWTs in httpOnly cookies
 * and refresh them, so there is no token handling here: same-origin
 * credentials, `Accept: application/json`, and hooks for 401/403.
 */
export function createApiClient(options: ApiClientOptions = {}) {
  const baseUrl = (options.baseUrl ?? "").replace(/\/+$/, "");
  const send = options.fetch ?? ((request: Request) => globalThis.fetch(request));

  async function fetchApi(request: Request): Promise<Response> {
    if (!request.headers.has("Accept")) request.headers.set("Accept", "application/json");
    const response = await send(request);
    if (response.status === 401) {
      options.onUnauthorized?.(response, request);
    } else if (response.status === 403 && options.onForbidden) {
      const body = await response
        .clone()
        .json()
        .catch(() => null);
      options.onForbidden(ApiError.fromResponse(response, body), request);
    }
    return response;
  }

  const client = createClient<paths>({
    baseUrl,
    credentials: "same-origin",
    headers: { Accept: "application/json" },
    fetch: fetchApi,
  });

  return Object.assign(client, {
    /** Same hooks, for requests that aren't JSON (CSV export, broadcasting auth). */
    rawFetch: (input: string, init?: RequestInit) =>
      fetchApi(new Request(`${baseUrl}${input}`, { credentials: "same-origin", ...init })),
  });
}

export type ApiClient = ReturnType<typeof createApiClient>;

type FetchResult<T> = { data?: T; error?: unknown; response: Response };

/**
 * Unwraps a typed client result: returns `data` or throws an {@link ApiError}
 * built from the Navree error body.
 */
export async function unwrap<T>(result: Promise<FetchResult<T>>): Promise<T> {
  const { data, error, response } = await result;
  if (!response.ok || error !== undefined) {
    throw ApiError.fromResponse(response, error);
  }
  return data as T;
}

/** Handlers the app installs at runtime (router redirect, toasts). */
export const apiEvents: {
  onUnauthorized?: ApiClientOptions["onUnauthorized"];
  onForbidden?: ApiClientOptions["onForbidden"];
} = {};

/** The app-wide client: same origin, through the BFF route handlers. */
export const api = createApiClient({
  onUnauthorized: (response, request) => apiEvents.onUnauthorized?.(response, request),
  onForbidden: (error, request) => apiEvents.onForbidden?.(error, request),
});
