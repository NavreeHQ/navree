"use client";

import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useEffect, type ReactNode } from "react";
import { AppSidebar } from "@/components/app-sidebar";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Separator } from "@/components/ui/separator";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";
import { Spinner } from "@/components/ui/spinner";
import { keys, useMe, useWorkspaceSlug, useWorkspaces } from "@/lib/queries";
import { listenToWorkspace } from "@/lib/realtime";
import { rememberWorkspace } from "@/lib/workspace";

/**
 * The shell of a workspace (`/w/<slug>/...`): the sidebar, the top bar and the screen. It
 * remembers the workspace for `/`, and refreshes the screens when a teammate changes a project
 * (real time, `private-workspace.{id}`).
 */
export default function WorkspaceLayout({ children }: { children: ReactNode }) {
  const slug = useWorkspaceSlug();
  const me = useMe();
  const workspaces = useWorkspaces();
  const queryClient = useQueryClient();
  const current = workspaces.data?.find((workspace) => workspace.slug === slug);

  useEffect(() => {
    if (current) rememberWorkspace(current.slug);
  }, [current]);

  const currentId = current?.id;
  useEffect(() => {
    if (!currentId) return;
    return listenToWorkspace(currentId, "project.changed", () => {
      void queryClient.invalidateQueries({ queryKey: keys.projects(slug) });
    });
  }, [currentId, slug, queryClient]);

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
  );
}
