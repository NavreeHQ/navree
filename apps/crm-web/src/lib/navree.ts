import { createNavree } from "@navree/next/server";

/**
 * The app's Navree: the backend-for-frontend behind `src/app/api` and
 * `src/app/broadcasting`, and the server-side helpers (`auth()`, `client()`).
 * Settings come from the `NAVREE_*` variables when the first request arrives, so
 * `NAVREE_API_URL` can change without a rebuild:
 *
 * - `NAVREE_API_URL`: where crm-api listens (default `http://127.0.0.1:8080`);
 * - `NAVREE_COOKIE_SECURE=0`: serve a production build over plain http;
 * - `APP_ORIGINS`: the public origin(s) behind a proxy that rewrites Host;
 * - `NAVREE_TRUSTED_PROXY_HOPS`, `NAVREE_CLIENT_IP_HEADER`: how the API learns the client's address.
 */
export const navree = createNavree();
