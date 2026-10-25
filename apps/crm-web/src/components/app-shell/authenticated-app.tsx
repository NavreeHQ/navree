"use client";

import { useAbility } from "@casl/react";
import { MoonIcon, RefreshCwIcon, SunIcon, UnplugIcon } from "lucide-react";
import { useTheme } from "next-themes";
import { useLayoutEffect, type ReactNode } from "react";
import { AppSidebar } from "@/components/app-shell/app-sidebar";
import { CommandSearch } from "@/components/app-shell/command-search";
import { NotificationsBell } from "@/components/app-shell/notifications-bell";
import { RealtimeBridge } from "@/components/app-shell/realtime-bridge";
import { UserMenu } from "@/components/app-shell/user-menu";
import { KeelMark } from "@/components/brand";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";
import { Skeleton } from "@/components/ui/skeleton";
import { useAbilityRules, useCurrentUser } from "@/features/auth/queries";
import { isApiError } from "@/lib/api/errors";
import type { User } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";

function ThemeToggle() {
  const { resolvedTheme, setTheme } = useTheme();
  const dark = resolvedTheme === "dark";
  return (
    <Button
      variant="ghost"
      size="icon"
      aria-label={dark ? "Switch to light mode" : "Switch to dark mode"}
      onClick={() => setTheme(dark ? "light" : "dark")}
    >
      <SunIcon className="hidden dark:block" />
      <MoonIcon className="dark:hidden" />
    </Button>
  );
}

function TopBar({ user }: { user: User }) {
  return (
    <header className="sticky top-0 z-20 flex h-14 shrink-0 items-center gap-3 border-b bg-background/90 px-4 backdrop-blur supports-[backdrop-filter]:bg-background/75 lg:px-6">
      <SidebarTrigger className="-ml-1" />
      <div className="flex min-w-0 flex-1">
        <CommandSearch />
      </div>
      <div className="flex items-center gap-1">
        <ThemeToggle />
        <NotificationsBell />
        <UserMenu user={user} />
      </div>
    </header>
  );
}

function ShellSkeleton() {
  return (
    <div className="flex min-h-svh" aria-busy="true" aria-label="Loading Keel CRM">
      <div className="hidden w-64 shrink-0 flex-col gap-3 bg-sidebar p-4 md:flex">
        <KeelMark className="text-sidebar-primary" />
        <div className="mt-4 flex flex-col gap-2.5">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-5 w-3/4 bg-sidebar-accent" />
          ))}
        </div>
      </div>
      <div className="flex flex-1 flex-col">
        <div className="h-14 border-b" />
        <div className="flex flex-col gap-4 p-6 lg:px-8">
          <Skeleton className="h-8 w-48" />
          <Skeleton className="h-4 w-72" />
          <Skeleton className="mt-4 h-64 w-full" />
        </div>
      </div>
    </div>
  );
}

function ApiUnavailable({ onRetry, detail }: { onRetry: () => void; detail?: string }) {
  return (
    <div className="flex min-h-svh items-center justify-center p-6">
      <Empty className="max-w-md border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <UnplugIcon />
          </EmptyMedia>
          <EmptyTitle>Can&apos;t reach the Keel API</EmptyTitle>
          <EmptyDescription>
            {detail ??
              "Start crm-api on 127.0.0.1:8080 (or set KEEL_API_URL), or run the app with NEXT_PUBLIC_API_MOCKS=1 to use demo data."}
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
        <div id="content" className="flex min-w-0 flex-1 flex-col px-4 py-6 lg:px-8">
          {children}
        </div>
      </SidebarInset>
      <RealtimeBridge userId={user.data.id} />
    </SidebarProvider>
  );
}
