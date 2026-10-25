"use client";

import { useAbility } from "@casl/react";
import {
  Building2Icon,
  ColumnsIcon,
  GaugeIcon,
  ListChecksIcon,
  ScrollTextIcon,
  SettingsIcon,
  UsersIcon,
  UserCogIcon,
} from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { KeelWordmark } from "@/components/brand";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  useSidebar,
} from "@/components/ui/sidebar";
import type { AppAbility } from "@/lib/casl/ability";
import { env } from "@/lib/env";
import type { User } from "@/lib/api/types";

const WORK = [
  { href: "/", label: "Dashboard", icon: GaugeIcon },
  { href: "/companies", label: "Companies", icon: Building2Icon },
  { href: "/contacts", label: "Contacts", icon: UsersIcon },
  { href: "/deals", label: "Deals", icon: ColumnsIcon },
  { href: "/tasks", label: "Tasks", icon: ListChecksIcon },
] as const;

/** Active item: a shoal-tint rule on the left edge, echoing the stage depth ramp. */
const ACTIVE_MARK = "data-[active=true]:shadow-[inset_2px_0_0_var(--sidebar-primary)]";

export function isActive(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}

export function AppSidebar({ user }: { user: User }) {
  const pathname = usePathname();
  const ability = useAbility<AppAbility>();
  const { setOpenMobile } = useSidebar();
  const isAdmin = user.roles.includes("admin");
  const canManageUsers = ability.can("create", "User");
  const closeMobile = () => setOpenMobile(false);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader className="h-14 justify-center px-3">
        <Link
          href="/"
          onClick={closeMobile}
          className="flex items-center rounded-md px-1 text-sidebar-accent-foreground"
        >
          <KeelWordmark className="group-data-[collapsible=icon]:[&>span]:hidden" />
        </Link>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              {WORK.map(({ href, label, icon: Icon }) => (
                <SidebarMenuItem key={href}>
                  <SidebarMenuButton
                    asChild
                    isActive={isActive(pathname, href)}
                    tooltip={label}
                    className={ACTIVE_MARK}
                  >
                    <Link href={href} onClick={closeMobile}>
                      <Icon />
                      <span>{label}</span>
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Workspace</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  asChild
                  isActive={pathname === "/settings"}
                  tooltip="Settings"
                  className={ACTIVE_MARK}
                >
                  <Link href="/settings" onClick={closeMobile}>
                    <SettingsIcon />
                    <span>Settings</span>
                  </Link>
                </SidebarMenuButton>
                {canManageUsers || isAdmin ? (
                  <SidebarMenuSub>
                    {canManageUsers ? (
                      <SidebarMenuSubItem>
                        <SidebarMenuSubButton asChild isActive={isActive(pathname, "/settings/users")}>
                          <Link href="/settings/users" onClick={closeMobile}>
                            <UserCogIcon />
                            <span>Users and roles</span>
                          </Link>
                        </SidebarMenuSubButton>
                      </SidebarMenuSubItem>
                    ) : null}
                    {isAdmin ? (
                      <SidebarMenuSubItem>
                        <SidebarMenuSubButton asChild isActive={isActive(pathname, "/settings/audit")}>
                          <Link href="/settings/audit" onClick={closeMobile}>
                            <ScrollTextIcon />
                            <span>Audit log</span>
                          </Link>
                        </SidebarMenuSubButton>
                      </SidebarMenuSubItem>
                    ) : null}
                  </SidebarMenuSub>
                ) : null}
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      {env.mocks ? (
        <SidebarFooter className="px-3 pb-3 group-data-[collapsible=icon]:hidden">
          <p className="rounded-md border border-sidebar-border px-2.5 py-2 text-xs leading-snug text-sidebar-foreground/75">
            Demo data from the mock API. Changes last until you close this tab.
          </p>
        </SidebarFooter>
      ) : null}
    </Sidebar>
  );
}
