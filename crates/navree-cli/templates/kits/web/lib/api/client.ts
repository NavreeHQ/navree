/**
 * The browser's API client. Every call is same-origin: the backend-for-frontend
 * (`@navree/next`, mounted in `app/api`) forwards it to the Navree API with the bearer token from the httpOnly
 * cookie. Calls carry `X-Workspace` with the workspace of the current `/w/<slug>/`
 * URL, which the API's `InWorkspace` guard resolves.
 */
import { ApiError } from "./errors";

let currentWorkspace: string | null = null;

/** Sets the workspace every following call is made in (the `/w/[workspace]` layout does). */
export function setWorkspace(slug: string | null): void {
  currentWorkspace = slug;
}

/** The workspace calls are made in. */
export function currentWorkspaceSlug(): string | null {
  return currentWorkspace;
}

/** Hooks for the app shell: a 401 outside the sign-in calls ends the session. */
export const apiEvents: { onUnauthorized?: () => void } = {};

export interface RequestOptions {
  /** Another workspace than the current one, or `null` for none. */
  workspace?: string | null;
  /** The fetch to use (tests). */
  fetch?: typeof fetch;
}

/** Calls `path` (`/api/...`) and returns the JSON body; throws an `ApiError` for any 4xx or 5xx. */
export async function request<T>(method: string, path: string, body?: unknown, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { accept: "application/json" };
  const workspace = options.workspace === undefined ? currentWorkspace : options.workspace;
  if (workspace) headers["x-workspace"] = workspace;
  if (body !== undefined) headers["content-type"] = "application/json";
  const send = options.fetch ?? fetch;
  const response = await send(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  if (!response.ok) {
    const error = await ApiError.from(response);
    // Wrong credentials and expired challenges are answers, not the end of a session.
    if (response.status === 401 && !path.startsWith("/api/auth/")) apiEvents.onUnauthorized?.();
    throw error;
  }
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

export const api = {
  get: <T>(path: string, options?: RequestOptions) => request<T>("GET", path, undefined, options),
  post: <T>(path: string, body?: unknown, options?: RequestOptions) => request<T>("POST", path, body, options),
  put: <T>(path: string, body?: unknown, options?: RequestOptions) => request<T>("PUT", path, body, options),
  patch: <T>(path: string, body?: unknown, options?: RequestOptions) => request<T>("PATCH", path, body, options),
  delete: <T = void>(path: string, options?: RequestOptions) => request<T>("DELETE", path, undefined, options),
};

/** The parts of a list request, in the one list grammar every Navree list reads. */
export interface ListParams {
  /** Each entry is sent as `filter[name]=value`; undefined, null and blank values are left out. */
  filter?: Record<string, string | number | boolean | null | undefined>;
  /** Fields to sort by, a leading `-` for descending: `-created_at,name`. */
  sort?: string;
  search?: string | null;
  page?: number;
  per_page?: number;
}

/**
 * The query string of a list request: `listQuery({ filter: { status: "active" }, search: "web", page: 2 })` is
 * `?filter%5Bstatus%5D=active&search=web&page=2`. A flat `?status=active` is not part of the grammar and the
 * API ignores it.
 */
export function listQuery({ filter, sort, search, page, per_page }: ListParams): string {
  const params = new URLSearchParams();
  for (const [name, value] of Object.entries(filter ?? {})) {
    if (value !== undefined && value !== null && value !== "") params.append(`filter[${name}]`, String(value));
  }
  if (sort) params.set("sort", sort);
  if (search?.trim()) params.set("search", search);
  if (page !== undefined) params.set("page", String(page));
  if (per_page !== undefined) params.set("per_page", String(per_page));
  const text = params.toString();
  return text ? `?${text}` : "";
}
