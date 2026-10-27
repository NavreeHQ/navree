import { createBff, type Bff } from "./bff";

let instance: Bff | undefined;

function flag(value: string | undefined): boolean | undefined {
  if (value === undefined || value === "") return undefined;
  return value === "1" || value === "true";
}

/**
 * The app's BFF, created on first use so the settings are read at runtime
 * (no rebuild to point at another API) and the single-flight refresh map is
 * shared by every request this server handles.
 *
 * Settings (the same names the Next.js front end uses; `NUXT_*` forms work too):
 * - `NAVREE_API_URL` / `NUXT_NAVREE_API_URL`: the Navree API (default http://127.0.0.1:8092);
 * - `NAVREE_COOKIE_SECURE=0|1`: the `Secure` flag on the token cookies (default: on in production);
 * - `APP_ORIGINS`: extra origins allowed to send mutations (the public URL behind a proxy);
 * - `NAVREE_TRUSTED_PROXY_HOPS`: reverse proxies in front of this server that append the client
 *   address to `X-Forwarded-For` (default 0: the browser's own header is never passed on).
 */
export function useBff(): Bff {
  if (instance) return instance;
  const config = useRuntimeConfig();
  const apiUrl = process.env.NAVREE_API_URL || config.navreeApiUrl || "http://127.0.0.1:8092";
  instance = createBff({
    apiUrl,
    secureCookies: flag(process.env.NAVREE_COOKIE_SECURE ?? process.env.NUXT_COOKIE_SECURE),
    allowedOrigins: (process.env.APP_ORIGINS ?? "")
      .split(",")
      .map((origin) => origin.trim())
      .filter(Boolean),
    trustedProxyHops: Number.parseInt(process.env.NAVREE_TRUSTED_PROXY_HOPS ?? "0", 10) || 0,
  });
  return instance;
}
