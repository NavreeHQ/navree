import type { BffOptions } from "./bff.js";

type Env = Record<string, string | undefined>;

function flag(value: string | undefined): boolean | undefined {
  if (value === undefined || value === "") return undefined;
  return value === "1" || value.toLowerCase() === "true";
}

/**
 * BFF options from environment variables, read at request time so the API
 * can move without a rebuild:
 *
 * | Variable | Meaning |
 * |---|---|
 * | `NAVREE_API_URL` (or `NUXT_NAVREE_API_URL`) | The API (default `http://127.0.0.1:8080`) |
 * | `NAVREE_COOKIE_SECURE` | `0`/`1`: the `Secure` flag (default: on in production) |
 * | `APP_ORIGINS` | Extra origins allowed to send non-GET requests, comma-separated |
 * | `NAVREE_TRUSTED_PROXY_HOPS` | Reverse proxies in front of the app that append the client to `X-Forwarded-For` |
 * | `NAVREE_CLIENT_IP_HEADER` | A header the proxy in front of the app sets to the bare client address (e.g. `x-real-ip`) |
 * | `NAVREE_WORKSPACE_HEADER` | The API's `workspace.header` (default `X-Workspace`) |
 */
export function bffOptionsFromEnv(env: Env = process.env, defaults: Partial<BffOptions> = {}): BffOptions {
  const options: BffOptions = {
    ...defaults,
    apiUrl: env.NAVREE_API_URL || env.NUXT_NAVREE_API_URL || defaults.apiUrl || "http://127.0.0.1:8080",
    trustedProxyHops: Number.parseInt(env.NAVREE_TRUSTED_PROXY_HOPS ?? "", 10) || defaults.trustedProxyHops || 0,
  };
  const secure = flag(env.NAVREE_COOKIE_SECURE ?? env.NUXT_NAVREE_COOKIE_SECURE);
  if (secure !== undefined) options.secureCookies = secure;
  const origins = (env.APP_ORIGINS ?? "")
    .split(",")
    .map((origin) => origin.trim())
    .filter(Boolean);
  if (origins.length > 0) options.allowedOrigins = origins;
  if (env.NAVREE_CLIENT_IP_HEADER) options.clientIpHeader = env.NAVREE_CLIENT_IP_HEADER;
  if (env.NAVREE_WORKSPACE_HEADER) options.workspaceHeader = env.NAVREE_WORKSPACE_HEADER;
  return options;
}
