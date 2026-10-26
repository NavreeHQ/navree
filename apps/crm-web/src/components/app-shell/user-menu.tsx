"use client";

import { useAbility } from "@casl/react";
import { LogOutIcon, SettingsIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { UserAvatar } from "@/components/user-avatar";
import { useLogout } from "@/features/auth/queries";
import type { User } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";

export function roleLabel(roles: string[]): string {
  if (roles.includes("admin")) return "Admin";
  if (roles.includes("manager")) return "Manager";
  if (roles.includes("rep")) return "Sales rep";
  return roles[0] ?? "Member";
}

export function UserMenu({ user }: { user: User }) {
  const router = useRouter();
  const ability = useAbility<AppAbility>();
  const logout = useLogout();

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
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          className="h-9 gap-2.5 rounded-full py-0 pr-1 pl-1 md:pr-3"
          aria-label={`Account menu for ${user.name}`}
        >
          <UserAvatar name={user.name} id={user.id} size="md" />
          <span className="hidden flex-col items-start leading-tight md:flex">
            <span className="text-[0.8125rem] font-medium text-fg">{user.name}</span>
            <span className="text-xs font-normal text-fg-3">{roleLabel(user.roles)}</span>
          </span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-60">
        <DropdownMenuLabel className="flex items-center gap-3 px-2 py-2">
          <UserAvatar name={user.name} id={user.id} size="md" />
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className="truncate text-sm font-medium text-fg">{user.name}</span>
            <span className="truncate text-xs font-normal text-fg-3">{user.email}</span>
          </span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem asChild>
            <Link href="/settings">
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
  );
}
