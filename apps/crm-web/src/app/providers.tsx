"use client";

import { AbilityProvider } from "@casl/react";
import { MutationCache, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "next-themes";
import { useRouter } from "next/navigation";
import { useEffect, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { apiEvents } from "@/lib/api/client";
import { errorMessage, isApiError } from "@/lib/api/errors";
import { keys } from "@/lib/api/keys";
import { createAppAbility } from "@/lib/casl/ability";
import { env } from "@/lib/env";

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 20_000,
        // Don't retry what the API has decided (4xx); retry transient failures twice.
        retry: (failureCount, error) => !(isApiError(error) && error.status < 500) && failureCount < 2,
      },
      mutations: { retry: false },
    },
    mutationCache: new MutationCache({
      onError: (error, _variables, _context, mutation) => {
        if (mutation.meta?.silent) return;
        // 401 redirects and 403 toasts are handled by the API client hooks below;
        // 400 validation errors are shown next to the form fields.
        if (isApiError(error) && (error.status === 401 || error.status === 403 || error.isValidation)) return;
        toast.error(errorMessage(error));
      },
    }),
  });
}

/** Maps API-level 401/403 to navigation and toasts. */
function ApiEventBridge({ queryClient }: { queryClient: QueryClient }) {
  const router = useRouter();
  useEffect(() => {
    apiEvents.onUnauthorized = () => {
      queryClient.setQueryData(keys.auth.user, null);
      const { pathname, search } = window.location;
      if (pathname !== "/login") {
        const next = pathname === "/" ? "" : `?next=${encodeURIComponent(pathname + search)}`;
        router.replace(`/login${next}`);
      }
    };
    apiEvents.onForbidden = (error) => {
      toast.error(errorMessage(error), { id: `forbidden:${error.messages[0] ?? ""}` });
    };
    return () => {
      apiEvents.onUnauthorized = undefined;
      apiEvents.onForbidden = undefined;
    };
  }, [queryClient, router]);
  return null;
}

/**
 * With `NEXT_PUBLIC_API_MOCKS=1` the API is served by an MSW service worker;
 * nothing renders until it is ready so the first requests are intercepted.
 */
function MockApiGate({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(!env.mocks);
  useEffect(() => {
    if (!env.mocks) return;
    let cancelled = false;
    import("@/mocks/browser")
      .then(({ startMockApi }) => startMockApi())
      .then(() => !cancelled && setReady(true))
      .catch((error: unknown) => {
        console.error("[navis] Could not start the mock API", error);
        if (!cancelled) setReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return ready ? children : null;
}

export function Providers({ children }: { children: ReactNode }) {
  const [queryClient] = useState(makeQueryClient);
  // One ability for the whole app; rules are loaded after sign-in with `ability.update()`.
  const [ability] = useState(() => createAppAbility());

  return (
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
      <QueryClientProvider client={queryClient}>
        <AbilityProvider value={ability}>
          <TooltipProvider delayDuration={300}>
            <ApiEventBridge queryClient={queryClient} />
            <MockApiGate>{children}</MockApiGate>
            <Toaster position="bottom-right" closeButton />
          </TooltipProvider>
        </AbilityProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
