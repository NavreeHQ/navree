import { isNavreeError } from "@navree/client";
import { currentUserLoader, SESSION_STALE_TIME } from "@/features/auth/queries";
import { keys } from "@/lib/api/keys";

/**
 * Every screen except /login needs a session. The server's session gate (the `@navree/nuxt`
 * module's) has already sent visitors without cookies to the login page; this loads the user into
 * the cache (cached by TanStack Query) through the module's sign-in state, which then loads their
 * CASL rules. No session (a 401 the BFF couldn't refresh) goes to /login with `next`. Anything
 * else (the API is down) lets the shell show "Can't reach the Navree API".
 */
export default defineNuxtRouteMiddleware(async (to) => {
  if (to.path === "/login") return;
  const { $queryClient } = useNuxtApp();
  try {
    await $queryClient.fetchQuery({
      queryKey: keys.auth.user,
      queryFn: currentUserLoader(),
      staleTime: SESSION_STALE_TIME,
      retry: false,
    });
  } catch (error) {
    if (isNavreeError(error) && error.status === 401) {
      return navigateTo(to.fullPath === "/" ? "/login" : `/login?next=${encodeURIComponent(to.fullPath)}`, { replace: true });
    }
  }
});
