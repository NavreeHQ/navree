import { fetchAbilityRules, fetchCurrentUser, SESSION_STALE_TIME } from "@/features/auth/queries";
import { isApiError } from "@/lib/api/errors";
import { keys } from "@/lib/api/keys";
import type { AppAbility } from "@/lib/casl/ability";

/**
 * Every screen except /login needs a session: load the user and their CASL
 * rules (cached by TanStack Query) and apply the rules before the screen
 * renders, so nothing ever shows a button the API would refuse. No session
 * (a 401 the BFF couldn't refresh) goes to /login with `next`. Anything else
 * (the API is down) lets the shell show "Can't reach the Navree API".
 */
export default defineNuxtRouteMiddleware(async (to) => {
  if (to.path === "/login") return;
  const { $queryClient, $ability } = useNuxtApp();
  try {
    await $queryClient.fetchQuery({ queryKey: keys.auth.user, queryFn: fetchCurrentUser, staleTime: SESSION_STALE_TIME, retry: false });
    const rules = await $queryClient.fetchQuery({
      queryKey: keys.auth.abilities,
      queryFn: fetchAbilityRules,
      staleTime: SESSION_STALE_TIME,
    });
    if ((($ability as AppAbility).rules as unknown) !== rules) {
      ($ability as AppAbility).update(rules as Parameters<AppAbility["update"]>[0]);
    }
  } catch (error) {
    if (isApiError(error) && error.status === 401) {
      return navigateTo(to.fullPath === "/" ? "/login" : `/login?next=${encodeURIComponent(to.fullPath)}`, { replace: true });
    }
  }
});
