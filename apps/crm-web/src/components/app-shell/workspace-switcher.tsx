"use client";

import { CheckIcon, ChevronsUpDownIcon, SearchIcon, SettingsIcon } from "lucide-react";
import Link from "next/link";
import { useState } from "react";
import { openCommandMenu } from "@/components/app-shell/command-search";
import { NavreeTile } from "@/components/brand";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from "@/components/ui/sidebar";
import { env } from "@/lib/env";

const isMac = () => typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/**
 * The sidebar header (sidebar-07's team switcher): the app and workspace you are
 * in, with a menu for workspace settings and the command menu. With the mock API
 * it says so: the data is a demo and lives in this tab.
 */
export function WorkspaceSwitcher() {
  const { isMobile, setOpenMobile } = useSidebar();
  // Rendered only in the browser (behind sign-in), so the platform is known on first render.
  const [mod] = useState(() => (isMac() ? "⌘" : "Ctrl"));
  const context = env.mocks ? "Demo workspace" : "Sales workspace";

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              className="gap-2 data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
              aria-label="Workspace menu"
            >
              <NavreeTile />
              <span className="grid min-w-0 flex-1 text-left leading-tight">
                <span className="truncate text-sm font-medium text-fg">Navree CRM</span>
                <span className="truncate text-xs text-fg-3">{context}</span>
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
            <DropdownMenuLabel className="text-xs font-normal text-fg-3">Workspace</DropdownMenuLabel>
            <DropdownMenuItem className="gap-2 p-1.5">
              <NavreeTile size="sm" />
              <span className="flex min-w-0 flex-col leading-tight">
                <span className="truncate font-medium text-fg">Navree CRM</span>
                <span className="truncate text-xs text-fg-3">{context}</span>
              </span>
              <CheckIcon className="ml-auto text-fg-2" aria-label="Current workspace" />
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem asChild>
                <Link href="/settings" onClick={() => setOpenMobile(false)}>
                  <SettingsIcon />
                  Workspace settings
                </Link>
              </DropdownMenuItem>
              <DropdownMenuItem
                onSelect={() => {
                  setOpenMobile(false);
                  openCommandMenu();
                }}
              >
                <SearchIcon />
                Search and commands
                <DropdownMenuShortcut>{mod} K</DropdownMenuShortcut>
              </DropdownMenuItem>
            </DropdownMenuGroup>
            {env.mocks ? (
              <>
                <DropdownMenuSeparator />
                <p className="flex gap-2 px-2 py-1.5 text-xs leading-snug text-fg-3">
                  <span aria-hidden="true" className="mt-1 size-1.5 shrink-0 rounded-full bg-amber" />
                  Demo data from the mock API. Changes last until you close this tab.
                </p>
              </>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}
