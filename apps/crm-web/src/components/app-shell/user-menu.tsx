"use client";

import { useAbility } from "@navree/next/abilities";
import { ChevronsUpDownIcon, LogOutIcon, SettingsIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from "@/components/ui/sidebar";
import { UserAvatar } from "@/components/user-avatar";
import { useLogout } from "@/features/auth/queries";
import type { User } from "@/lib/api/types";

export function roleLabel(roles: string[]): string {
  if (roles.includes("admin")) return "Admin";
  if (roles.includes("manager")) return "Manager";
  if (roles.includes("rep")) return "Sales rep";
  return roles[0] ?? "Member";
}

function Identity({ user }: { user: User }) {
  return (
    <>
      <UserAvatar name={user.name} id={user.id} size="md" />
      <span className="grid min-w-0 flex-1 text-left leading-tight">
        <span className="truncate text-sm font-medium text-fg">{user.name}</span>
        <span className="truncate text-xs text-fg-3">{user.email}</span>
      </span>
    </>
  );
}

/**
 * The sidebar footer (sidebar-07's NavUser): the signed-in person, and their
 * menu with settings and signing out.
 */
export function UserMenu({ user }: { user: User }) {
  const router = useRouter();
  const ability = useAbility();
  const logout = useLogout();
  const { isMobile, setOpenMobile } = useSidebar();

  const signOut = () => {
    logout.mutate(
      {},
      {
        onSettled: () => {
          ability.update([]);
          router.replace("/login");
        },
      },
    );
  };

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              className="gap-2 data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
              aria-label={`Account menu for ${user.name}`}
            >
              <Identity user={user} />
              <ChevronsUpDownIcon className="ml-auto text-fg-3" aria-hidden="true" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="w-(--radix-dropdown-menu-trigger-width) min-w-60"
            side={isMobile ? "top" : "right"}
            align="end"
            sideOffset={8}
          >
            <DropdownMenuLabel className="flex items-center gap-2 px-1.5 py-1.5 font-normal">
              <Identity user={user} />
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem asChild>
                <Link href="/settings" onClick={() => setOpenMobile(false)}>
                  <SettingsIcon />
                  Settings
                </Link>
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem onSelect={signOut} disabled={logout.isPending}>
                <LogOutIcon />
                Sign out
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}
