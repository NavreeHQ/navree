"use client";

import { MutationCache, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useEffect, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { apiEvents } from "@/lib/api/client";
import { errorMessage, isApiError } from "@/lib/api/errors";
import { keys } from "@/lib/queries";

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 20_000,
        // Don't retry what the API decided (4xx); retry transient failures twice.
        retry: (failureCount, error) => !(isApiError(error) && error.status < 500) && failureCount < 2,
      },
      mutations: { retry: false },
    },
    mutationCache: new MutationCache({
      onError: (error, _variables, _context, mutation) => {
        if (mutation.meta?.silent) return;
        // 401s go to the sign-in page; validation errors show under the form's fields.
        if (isApiError(error) && (error.status === 401 || error.isValidation)) return;
        toast.error(errorMessage(error));
      },
    }),
  });
}

/** A 401 from the API ends the session: back to the sign-in page, then here again. */
function SessionBridge({ queryClient }: { queryClient: QueryClient }) {
  const router = useRouter();
  useEffect(() => {
    apiEvents.onUnauthorized = () => {
      queryClient.setQueryData(keys.me, null);
      const { pathname, search } = window.location;
      router.replace(`/sign-in?next=${encodeURIComponent(pathname + search)}`);
    };
    return () => {
      apiEvents.onUnauthorized = undefined;
    };
  }, [queryClient, router]);
  return null;
}

export function Providers({ children }: { children: ReactNode }) {
  const [queryClient] = useState(makeQueryClient);
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={300}>
        <SessionBridge queryClient={queryClient} />
        {children}
        <Toaster position="bottom-right" closeButton />
      </TooltipProvider>
    </QueryClientProvider>
  );
}
