"use client";

import { RealtimeProvider, useChannel } from "@navree/next/realtime";
import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useEffect, type ReactNode } from "react";
import { AppSidebar } from "@/components/app-sidebar";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Separator } from "@/components/ui/separator";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";
import { Spinner } from "@/components/ui/spinner";
import { env } from "@/lib/env";
import { keys, useMe, useWorkspaceSlug, useWorkspaces } from "@/lib/queries";
import { rememberWorkspace } from "@/lib/workspace";

/** The API's WebSocket server (the Pusher protocol), as `.env.example` describes it. */
const REALTIME_SERVER = {
  key: env.realtime.key,
  wsHost: env.realtime.host,
  wsPort: env.realtime.port,
  forceTLS: env.realtime.tls,
};

/**
 * Refreshes the project screens when a teammate changes a project (`private-workspace.<id>`, the
 * API's `ChannelsModule`). Real time is optional: when the socket can't connect, screens still
 * refetch on focus.
 */
function WorkspaceLive({ id, slug }: { id: string; slug: string }) {
  const queryClient = useQueryClient();
  useChannel(`workspace.${id}`, "project.changed", () => {
    void queryClient.invalidateQueries({ queryKey: keys.projects(slug) });
  });
  return null;
}

/**
 * The shell of a workspace (`/w/<slug>/...`): the sidebar, the top bar and the screen. It
 * remembers the workspace for `/`, and keeps the screens live (real time).
 */
export default function WorkspaceLayout({ children }: { children: ReactNode }) {
  const slug = useWorkspaceSlug();
  const me = useMe();
  const workspaces = useWorkspaces();
  const current = workspaces.data?.find((workspace) => workspace.slug === slug);

  useEffect(() => {
    if (current) rememberWorkspace(current.slug);
  }, [current]);

  if (!me.data || workspaces.isPending) {
    return (
      <main id="content" className="flex min-h-svh items-center justify-center" aria-busy="true">
        <Spinner className="size-5 text-fg-3" />
      </main>
    );
  }

  if (!current) {
    return (
      <main id="content" className="flex min-h-svh items-center justify-center p-6">
        <Empty className="surface max-w-md flex-none p-8">
          <EmptyHeader>
            <EmptyTitle>Workspace not found</EmptyTitle>
            <EmptyDescription>It doesn&apos;t exist, or you are not a member of it.</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button variant="outline" asChild>
              <Link href="/">Go to your workspaces</Link>
            </Button>
          </EmptyContent>
        </Empty>
      </main>
    );
  }

  return (
    <RealtimeProvider enabled={Boolean(env.realtime.key)} server={REALTIME_SERVER}>
      <WorkspaceLive id={current.id} slug={slug} />
      <SidebarProvider>
        <AppSidebar user={me.data} current={current} workspaces={workspaces.data ?? []} />
        <SidebarInset className="min-w-0">
          <header className="flex h-(--header-h) shrink-0 items-center gap-2 border-b px-4">
            <SidebarTrigger className="-ml-1" />
            <Separator orientation="vertical" className="mr-2 data-[orientation=vertical]:h-4" />
            <span className="truncate text-sm text-fg-2">{current.name}</span>
          </header>
          <div id="content" tabIndex={-1} className="flex min-w-0 flex-1 flex-col gap-6 p-4 outline-none sm:p-6">
            {children}
          </div>
        </SidebarInset>
      </SidebarProvider>
    </RealtimeProvider>
  );
}
