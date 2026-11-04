import { createNavreeClient, unwrap, type NavreeClientOptions } from "@navree/client";
import type { paths } from "./types";

export { unwrap };

/**
 * Builds the typed client the browser uses: `@navree/client`'s client on this app's `paths`. It only
 * ever calls the app's own `/api/*` routes (the BFF the `@navree/nuxt` module serves), which hold
 * the JWTs in httpOnly cookies and refresh them, so there is no token handling here: same-origin
 * credentials, `Accept: application/json`, a request id, and hooks for 401 and 403.
 */
export function createApiClient(options: NavreeClientOptions = {}) {
  return createNavreeClient<paths>(options);
}

export type ApiClient = ReturnType<typeof createApiClient>;

/** Handlers the app installs at runtime (router redirect, toasts). */
export const apiEvents: {
  onUnauthorized?: NavreeClientOptions["onUnauthorized"];
  onForbidden?: NavreeClientOptions["onForbidden"];
} = {};

/**
 * The app-wide client: same origin, through the BFF. TanStack Query runs its query functions
 * outside any component, so the app keeps one client here instead of asking for `useNavreeClient()`.
 */
export const api = createApiClient({
  onUnauthorized: (response, request) => apiEvents.onUnauthorized?.(response, request),
  onForbidden: (error, request) => apiEvents.onForbidden?.(error, request),
});
