"use client";

import { useQueryClient } from "@tanstack/react-query";
import { ChevronsUpDownIcon, LogOutIcon, ShieldIcon, UserRoundIcon, WrenchIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from "@/components/ui/sidebar";
import { api } from "@/lib/api/client";
import type { User } from "@/lib/api/types";
import { useGlobalAbility } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

/** The sidebar footer: who is signed in, their account settings, the admin panel and sign-out. */
export function UserMenu({ user, workspace }: { user: User; workspace: string }) {
  const router = useRouter();
  const queryClient = useQueryClient();
  const { isMobile, setOpenMobile } = useSidebar();
  const global = useGlobalAbility();
  const close = () => setOpenMobile(false);

  async function signOut() {
    await api.post("/api/auth/logout", undefined, { workspace: null }).catch(() => undefined);
    queryClient.clear();
    router.replace("/sign-in");
  }

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton size="lg" aria-label="Account menu">
              <UserRoundIcon className="text-fg-2" aria-hidden="true" />
              <span className="grid min-w-0 flex-1 text-left leading-tight">
                <span className="truncate text-sm font-medium text-fg">{user.name}</span>
                <span className="truncate text-xs text-fg-3">{user.email}</span>
              </span>
              <ChevronsUpDownIcon className="ml-auto text-fg-3" aria-hidden="true" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="w-(--radix-dropdown-menu-trigger-width) min-w-56"
            align="end"
            side={isMobile ? "bottom" : "right"}
            sideOffset={8}
          >
            <DropdownMenuLabel className="text-xs font-normal text-fg-3">{user.email}</DropdownMenuLabel>
            <DropdownMenuItem asChild>
              <Link href={workspacePath(workspace, "/settings/profile")} onClick={close}>
                <UserRoundIcon />
                Profile
              </Link>
            </DropdownMenuItem>
            <DropdownMenuItem asChild>
              <Link href={workspacePath(workspace, "/settings/security")} onClick={close}>
                <ShieldIcon />
                Password and security
              </Link>
            </DropdownMenuItem>
            {global.can("manage", "all") ? (
              <DropdownMenuItem asChild>
                {/* Helm, served by the API through the BFF: a full page load, not a Next.js page. */}
                {/* eslint-disable-next-line @next/next/no-html-link-for-pages */}
                <a href="/admin">
                  <WrenchIcon />
                  Admin panel
                </a>
              </DropdownMenuItem>
            ) : null}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => void signOut()}>
              <LogOutIcon />
              Sign out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}
