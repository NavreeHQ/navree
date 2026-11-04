import { errorMessage, isNavreeError } from "@navree/client";
import { MutationCache, QueryClient, VueQueryPlugin } from "@tanstack/vue-query";
import { toast } from "vue-sonner";

/**
 * TanStack Query for Vue: one client for the app. Queries don't retry what the
 * API decided (4xx); mutation errors become toasts unless they are handled
 * elsewhere (401 redirects, 403 toasts from the API client hooks, and 400
 * validation errors, which forms show next to their fields).
 */
export default defineNuxtPlugin({
  name: "navree-query",
  setup(nuxtApp) {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          staleTime: 20_000,
          retry: (failureCount, error) => {
            const decidedByApi = isNavreeError(error) && error.status >= 400 && error.status < 500;
            return !decidedByApi && failureCount < 2;
          },
        },
        mutations: { retry: false },
      },
      mutationCache: new MutationCache({
        onError: (error, _variables, _context, mutation) => {
          if (mutation.meta?.silent) return;
          if (isNavreeError(error) && (error.status === 401 || error.status === 403 || error.isValidation)) return;
          toast.error(errorMessage(error));
        },
      }),
    });
    nuxtApp.vueApp.use(VueQueryPlugin, { queryClient });
    return { provide: { queryClient } };
  },
});
