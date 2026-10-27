import { ACCESS_COOKIE, REFRESH_COOKIE } from "../lib/bff";

/** Paths that are never app screens: the BFF, assets, the service worker and the login page itself. */
const PASS_THROUGH = /^\/(api|broadcasting|_nuxt|__nuxt|_ipx|login)(\/|$)|\.[a-z0-9]+$/i;

/**
 * Screens behind sign-in: a browser with neither token cookie has no session,
 * so it goes straight to `/login?next=...` before the app shell loads. Whether
 * a cookie is still valid is the API's call; the app redirects on a 401 too.
 * Off with the mock API (its session lives in the tab, not in cookies).
 */
export default defineEventHandler((event) => {
  if (event.method !== "GET" && event.method !== "HEAD") return;
  const url = getRequestURL(event);
  if (PASS_THROUGH.test(url.pathname)) return;
  if (!(getRequestHeader(event, "accept") ?? "").includes("text/html")) return;
  if (useRuntimeConfig(event).public.apiMocks) return;
  if (getCookie(event, ACCESS_COOKIE) || getCookie(event, REFRESH_COOKIE)) return;
  const target = url.pathname + url.search;
  return sendRedirect(event, target === "/" ? "/login" : `/login?next=${encodeURIComponent(target)}`, 302);
});
