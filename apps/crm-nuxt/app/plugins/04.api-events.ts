import { errorMessage } from "@navree/client";
import { toast } from "vue-sonner";
import { apiEvents } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";

/**
 * Maps API-level outcomes to the app: a 401 (the BFF couldn't refresh the
 * session) clears the cached user, ends the module's sign-in state and goes to
 * the login page, keeping the current address as `next`; a 403 becomes one
 * toast per message.
 */
export default defineNuxtPlugin({
  name: "navree-api-events",
  dependsOn: ["navree-query"],
  setup(nuxtApp) {
    const router = useRouter();
    // Taken here, inside the plugin's setup: the handler below runs outside Nuxt's context.
    const auth = useNavreeAuth();
    apiEvents.onUnauthorized = () => {
      nuxtApp.$queryClient.setQueryData(keys.auth.user, null);
      const { pathname, search } = window.location;
      if (pathname === "/login") return;
      const next = pathname === "/" ? "" : `?next=${encodeURIComponent(pathname + search)}`;
      // The module's sign-in state still holds the user: asking again ends it (the session is gone) and drops
      // their abilities, so the next person to sign in does not start with them.
      void auth
        .load()
        .catch(() => undefined)
        .finally(() => router.replace(`/login${next}`));
    };
    apiEvents.onForbidden = (error) => {
      toast.error(errorMessage(error), { id: `forbidden:${error.messages[0] ?? ""}` });
    };
  },
});
