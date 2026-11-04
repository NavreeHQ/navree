"use client";

import { AbilityProvider } from "@navree/next/abilities";
import { RealtimeProvider } from "@navree/next/realtime";
import { RefreshCwIcon, UnplugIcon } from "lucide-react";
import type { ReactNode } from "react";
import { AppSidebar } from "@/components/app-shell/app-sidebar";
import { RealtimeBridge } from "@/components/app-shell/realtime-bridge";
import { ShellSlotsProvider } from "@/components/app-shell/shell-slots";
import { TopBar } from "@/components/app-shell/top-bar";
import { NavreeWordmark } from "@/components/brand";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { Skeleton } from "@/components/ui/skeleton";
import { useAbilityRules, useCurrentUser } from "@/features/auth/queries";
import { isApiError } from "@/lib/api/errors";
import { env } from "@/lib/env";

/** The shell's outline while the user and their permissions load (same widths as the real one). */
function ShellSkeleton({ sidebarOpen }: { sidebarOpen: boolean }) {
  return (
    <div className="flex min-h-svh" aria-busy="true" aria-label="Loading Navree CRM">
      <div
        className={
          sidebarOpen
            ? "hidden w-64 shrink-0 flex-col gap-2 border-r bg-sidebar p-2 md:flex"
            : "hidden w-12 shrink-0 flex-col gap-2 border-r bg-sidebar p-2 md:flex"
        }
      >
        <Skeleton className="h-12 bg-ink-2" />
        <div className="mt-4 flex flex-col gap-2 px-2">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-4 bg-ink-2" style={{ width: sidebarOpen ? `${78 - i * 7}%` : "100%" }} />
          ))}
        </div>
      </div>
      <div className="flex flex-1 flex-col">
        <div className="h-(--header-h) border-b" />
        <div className="flex flex-col gap-4 p-4 sm:p-6">
          <Skeleton className="h-7 w-48" />
          <Skeleton className="h-4 w-72 max-w-full" />
          <Skeleton className="mt-2 h-24 w-full rounded-xl" />
          <div className="grid gap-4 lg:grid-cols-5">
            <Skeleton className="h-72 rounded-xl lg:col-span-3" />
            <Skeleton className="h-72 rounded-xl lg:col-span-2" />
          </div>
        </div>
      </div>
    </div>
  );
}

function ApiUnavailable({ onRetry, detail }: { onRetry: () => void; detail?: string }) {
  return (
    <div className="flex min-h-svh flex-col items-center justify-center gap-8 p-6">
      <NavreeWordmark />
      <Empty className="surface max-w-md flex-none border-solid p-8">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <UnplugIcon />
          </EmptyMedia>
          <EmptyTitle>Can&apos;t reach the Navree API</EmptyTitle>
          <EmptyDescription>
            {detail ??
              "Start crm-api on 127.0.0.1:8080 (or set NAVREE_API_URL), or run the app with NEXT_PUBLIC_API_MOCKS=1 to use demo data."}
          </EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button variant="outline" onClick={onRetry}>
            <RefreshCwIcon data-icon="inline-start" />
            Try again
          </Button>
        </EmptyContent>
      </Empty>
    </div>
  );
}

/** Where the API's real-time server is (the Pusher protocol), and the key it expects. */
const REALTIME_SERVER = { key: env.pusherKey, wsHost: env.wsHost, wsPort: env.wsPort };

/** With the mock API the "socket" is a window event bus; loaded only then, so mocks never ship in production. */
const connectMockBus = async () => (await import("@/lib/realtime/mock-bus")).connectMockBus();

/**
 * Everything behind sign-in: loads the user (401 → /login via the API client),
 * gives the app their CASL rules as one ability before rendering, opens
 * real-time, and renders the shell.
 */
export function AuthenticatedApp({ children, sidebarOpen = true }: { children: ReactNode; sidebarOpen?: boolean }) {
  const user = useCurrentUser();
  const rules = useAbilityRules(Boolean(user.data));

  const unauthenticated = isApiError(user.error) && user.error.status === 401;
  if (user.isError && !unauthenticated) {
    return <ApiUnavailable onRetry={() => void user.refetch()} />;
  }
  if (rules.isError) {
    return <ApiUnavailable onRetry={() => void rules.refetch()} detail="Your permissions couldn't be loaded." />;
  }
  if (!user.data || !rules.data) {
    return <ShellSkeleton sidebarOpen={sidebarOpen} />;
  }

  return (
    <AbilityProvider rules={rules.data}>
      <RealtimeProvider
        enabled={env.mocks || env.realtime}
        server={env.mocks ? undefined : REALTIME_SERVER}
        connect={env.mocks ? connectMockBus : undefined}
      >
        <SidebarProvider defaultOpen={sidebarOpen}>
          <ShellSlotsProvider>
            <AppSidebar user={user.data} />
            <SidebarInset className="min-w-0">
              <TopBar />
              {/* SidebarInset is the <main> landmark. */}
              <div id="content" tabIndex={-1} className="flex min-w-0 flex-1 flex-col p-4 outline-none sm:p-6">
                {children}
              </div>
            </SidebarInset>
          </ShellSlotsProvider>
          <RealtimeBridge userId={user.data.id} />
        </SidebarProvider>
      </RealtimeProvider>
    </AbilityProvider>
  );
}
