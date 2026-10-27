"use client";

import { Building2Icon, CheckIcon, ChevronsUpDownIcon, PlusIcon, SettingsIcon } from "lucide-react";
import Link from "next/link";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from "@/components/ui/sidebar";
import type { Workspace } from "@/lib/api/types";
import { APP_NAME } from "@/lib/app";
import { workspacePath } from "@/lib/workspace";

/** The sidebar header: the workspace you are in, and every other one you belong to. */
export function WorkspaceSwitcher({ current, workspaces }: { current: Workspace; workspaces: Workspace[] }) {
  const { isMobile, setOpenMobile } = useSidebar();
  const close = () => setOpenMobile(false);

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              className="gap-2 data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
              aria-label="Switch workspace"
            >
              <Building2Icon className="text-fg-2" aria-hidden="true" />
              <span className="grid min-w-0 flex-1 text-left leading-tight">
                <span className="truncate text-sm font-medium text-fg">{current.name}</span>
                <span className="truncate text-xs text-fg-3">{APP_NAME}</span>
              </span>
              <ChevronsUpDownIcon className="ml-auto text-fg-3" aria-hidden="true" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="w-(--radix-dropdown-menu-trigger-width) min-w-60"
            align="start"
            side={isMobile ? "bottom" : "right"}
            sideOffset={8}
          >
            <DropdownMenuLabel className="text-xs font-normal text-fg-3">Workspaces</DropdownMenuLabel>
            {workspaces.map((workspace) => (
              <DropdownMenuItem key={workspace.id} asChild>
                <Link href={workspacePath(workspace.slug)} onClick={close}>
                  <span className="flex min-w-0 flex-col leading-tight">
                    <span className="truncate font-medium text-fg">{workspace.name}</span>
                    <span className="truncate text-xs text-fg-3 capitalize">{workspace.role}</span>
                  </span>
                  {workspace.id === current.id ? (
                    <CheckIcon className="ml-auto text-fg-2" aria-label="Current workspace" />
                  ) : null}
                </Link>
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem asChild>
              <Link href="/onboarding" onClick={close}>
                <PlusIcon />
                Create a workspace
              </Link>
            </DropdownMenuItem>
            <DropdownMenuItem asChild>
              <Link href={workspacePath(current.slug, "/settings")} onClick={close}>
                <SettingsIcon />
                Workspace settings
              </Link>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}
