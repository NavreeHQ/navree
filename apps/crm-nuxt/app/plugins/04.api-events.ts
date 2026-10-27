import { toast } from "vue-sonner";
import { apiEvents } from "@/lib/api/client";
import { errorMessage } from "@/lib/api/errors";
import { keys } from "@/lib/api/keys";

/**
 * Maps API-level outcomes to the app: a 401 (the BFF couldn't refresh the
 * session) clears the cached user and goes to the login page, keeping the
 * current address as `next`; a 403 becomes one toast per message.
 */
export default defineNuxtPlugin({
  name: "navree-api-events",
  dependsOn: ["navree-query"],
  setup(nuxtApp) {
    const router = useRouter();
    apiEvents.onUnauthorized = () => {
      nuxtApp.$queryClient.setQueryData(keys.auth.user, null);
      const { pathname, search } = window.location;
      if (pathname !== "/login") {
        const next = pathname === "/" ? "" : `?next=${encodeURIComponent(pathname + search)}`;
        void router.replace(`/login${next}`);
      }
    };
    apiEvents.onForbidden = (error) => {
      toast.error(errorMessage(error), { id: `forbidden:${error.messages[0] ?? ""}` });
    };
  },
});
