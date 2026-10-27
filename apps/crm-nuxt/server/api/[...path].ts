import { useBff } from "../lib/instance";

/**
 * Every `/api/*` call from the browser goes to Navree with the bearer token
 * from the httpOnly cookie (refreshed once on a 401). Mutations must come from
 * this app's own origin.
 */
export default defineEventHandler((event) => {
  const request = toWebRequest(event);
  return useBff().proxy(request, new URL(request.url).pathname);
});
