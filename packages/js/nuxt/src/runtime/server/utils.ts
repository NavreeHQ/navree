/** Server utilities the module auto-imports into Nitro: the app's BFF and the request's session. */
import { bffOptionsFromEnv } from "@navree/bff";
import type { H3Event } from "h3";
import { useRuntimeConfig } from "#imports";
import { bffResolver, getNavreeSession as sessionOf } from "../../server/handlers.js";

/**
 * The app's BFF. Settings: `NAVREE_API_URL` (or `NUXT_NAVREE_API_URL`),
 * `NAVREE_COOKIE_SECURE`, `APP_ORIGINS`, `NAVREE_TRUSTED_PROXY_HOPS`, then
 * the module's options.
 */
export const useNavreeBff = bffResolver((event?: H3Event) => {
  const config = useRuntimeConfig(event).navree;
  return bffOptionsFromEnv(process.env, {
    apiUrl: config.apiUrl || "http://127.0.0.1:8080",
    workspaceHeader: config.workspaceHeader,
    pages: config.pages,
    socialPaths: config.socialPaths,
    redirectNavigations: config.redirectNavigations,
  });
});

/** The signed-in user, token and workspace of a request (`null` when signed out). */
export function getNavreeSession<User = unknown>(event: H3Event) {
  return sessionOf<User>(event, useNavreeBff(event));
}
