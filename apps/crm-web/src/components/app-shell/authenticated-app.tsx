"use client";

import { useAbility } from "@casl/react";
import { RefreshCwIcon, UnplugIcon } from "lucide-react";
import Link from "next/link";
import { useLayoutEffect, type ReactNode } from "react";
import { AppSidebar } from "@/components/app-shell/app-sidebar";
import { CommandSearch } from "@/components/app-shell/command-search";
import { NotificationsBell } from "@/components/app-shell/notifications-bell";
import { RealtimeBridge } from "@/components/app-shell/realtime-bridge";
import { UserMenu } from "@/components/app-shell/user-menu";
import { NavreeMark, NavreeWordmark } from "@/components/brand";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";
import { Skeleton } from "@/components/ui/skeleton";
import { useAbilityRules, useCurrentUser } from "@/features/auth/queries";
import { isApiError } from "@/lib/api/errors";
import type { User } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";

function TopBar({ user }: { user: User }) {
  return (
    <header className="sticky top-0 z-20 flex h-(--header-h) shrink-0 items-center gap-2 border-b bg-ink-0/80 px-3 backdrop-blur-md supports-[backdrop-filter]:bg-ink-0/70 sm:gap-3 sm:px-4 lg:px-6">
      <SidebarTrigger className="text-fg-2" />
      <Link href="/" aria-label="Navree CRM home" className="rounded-md md:hidden">
        <NavreeMark className="size-6" />
      </Link>
      <div className="flex min-w-0 flex-1">
        <CommandSearch />
      </div>
      <div className="flex items-center gap-1">
        <NotificationsBell />
        <UserMenu user={user} />
      </div>
    </header>
  );
}

function ShellSkeleton() {
  return (
    <div className="flex min-h-svh" aria-busy="true" aria-label="Loading Navree CRM">
      <div className="hidden w-64 shrink-0 flex-col gap-3 border-r bg-sidebar p-4 md:flex">
        <NavreeWordmark />
        <div className="mt-6 flex flex-col gap-3">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-5 bg-ink-2" style={{ width: `${78 - i * 7}%` }} />
          ))}
        </div>
      </div>
      <div className="flex flex-1 flex-col">
        <div className="h-(--header-h) border-b" />
        <div className="flex flex-col gap-4 px-4 py-8 lg:px-8">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-8 w-56" />
          <Skeleton className="mt-4 h-28 w-full rounded-xl" />
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
      <Empty className="surface max-w-md border-solid p-8">
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

/**
 * Everything behind sign-in: loads the user (401 → /login via the API client),
 * applies their CASL rules to the app-wide ability before rendering, connects
 * real-time, and renders the shell.
 */
export function AuthenticatedApp({ children }: { children: ReactNode }) {
  const user = useCurrentUser();
  const rules = useAbilityRules(Boolean(user.data));
  // useAbility re-renders on every `ability.update()`, so "applied" is derived from the ability itself.
  const ability = useAbility<AppAbility>();
  const applied = rules.data !== undefined && (ability.rules as unknown) === rules.data;

  useLayoutEffect(() => {
    if (rules.data && (ability.rules as unknown) !== rules.data) {
      ability.update(rules.data as Parameters<AppAbility["update"]>[0]);
    }
  }, [ability, rules.data]);

  const unauthenticated = isApiError(user.error) && user.error.status === 401;
  if (user.isError && !unauthenticated) {
    return <ApiUnavailable onRetry={() => void user.refetch()} />;
  }
  if (rules.isError) {
    return <ApiUnavailable onRetry={() => void rules.refetch()} detail="Your permissions couldn't be loaded." />;
  }
  if (!user.data || !rules.data || !applied) {
    return <ShellSkeleton />;
  }

  return (
    <SidebarProvider>
      <AppSidebar user={user.data} />
      <SidebarInset className="min-w-0 bg-background">
        <TopBar user={user.data} />
        {/* SidebarInset is the <main> landmark. */}
        <div
          id="content"
          tabIndex={-1}
          className="flex min-w-0 flex-1 flex-col px-4 py-6 outline-none sm:px-6 lg:px-8 lg:py-8"
        >
          {children}
        </div>
      </SidebarInset>
      <RealtimeBridge userId={user.data.id} />
    </SidebarProvider>
  );
}
