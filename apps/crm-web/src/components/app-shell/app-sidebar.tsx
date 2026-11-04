"use client";

import { useAbility } from "@navree/next/abilities";
import {
  Building2Icon,
  ChevronRightIcon,
  GaugeIcon,
  KanbanIcon,
  ListChecksIcon,
  SettingsIcon,
  UsersIcon,
} from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { UserMenu } from "@/components/app-shell/user-menu";
import { WorkspaceSwitcher } from "@/components/app-shell/workspace-switcher";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar";
import type { User } from "@/lib/api/types";

export const SALES = [
  { href: "/", label: "Dashboard", icon: GaugeIcon },
  { href: "/companies", label: "Companies", icon: Building2Icon },
  { href: "/contacts", label: "Contacts", icon: UsersIcon },
  { href: "/deals", label: "Deals", icon: KanbanIcon },
  { href: "/tasks", label: "Tasks", icon: ListChecksIcon },
] as const;

export function isActive(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}

/**
 * The app sidebar, the shadcn/ui `sidebar-07` layout (DESIGN.md section 9): the
 * workspace switcher, the Sales and Workspace groups, the user menu in the
 * footer. Collapses to icons (Ctrl/Cmd+B, the rail, the top bar trigger) and
 * becomes a sheet on phones.
 */
export function AppSidebar({ user }: { user: User }) {
  const pathname = usePathname();
  const ability = useAbility();
  const { setOpenMobile } = useSidebar();
  const closeMobile = () => setOpenMobile(false);

  const settingsPages = [
    ability.can("create", "User") ? { href: "/settings/users", label: "Users and roles" } : null,
    user.roles.includes("admin") ? { href: "/settings/audit", label: "Audit log" } : null,
  ].filter((p) => p !== null);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <WorkspaceSwitcher />
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Sales</SidebarGroupLabel>
          <SidebarMenu>
            {SALES.map(({ href, label, icon: Icon }) => (
              <SidebarMenuItem key={href}>
                <SidebarMenuButton asChild isActive={isActive(pathname, href)} tooltip={label}>
                  <Link href={href} onClick={closeMobile} aria-current={isActive(pathname, href) ? "page" : undefined}>
                    <Icon />
                    <span>{label}</span>
                  </Link>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Workspace</SidebarGroupLabel>
          <SidebarMenu>
            <Collapsible asChild defaultOpen className="group/collapsible">
              <SidebarMenuItem>
                <SidebarMenuButton asChild isActive={pathname === "/settings"} tooltip="Settings">
                  <Link
                    href="/settings"
                    onClick={closeMobile}
                    aria-current={pathname === "/settings" ? "page" : undefined}
                  >
                    <SettingsIcon />
                    <span>Settings</span>
                  </Link>
                </SidebarMenuButton>
                {settingsPages.length ? (
                  <>
                    <CollapsibleTrigger asChild>
                      <SidebarMenuAction className="text-fg-3 data-[state=open]:rotate-90">
                        <ChevronRightIcon />
                        <span className="sr-only">Show or hide settings pages</span>
                      </SidebarMenuAction>
                    </CollapsibleTrigger>
                    <CollapsibleContent>
                      <SidebarMenuSub>
                        {settingsPages.map((page) => (
                          <SidebarMenuSubItem key={page.href}>
                            <SidebarMenuSubButton asChild isActive={isActive(pathname, page.href)}>
                              <Link
                                href={page.href}
                                onClick={closeMobile}
                                aria-current={isActive(pathname, page.href) ? "page" : undefined}
                              >
                                <span>{page.label}</span>
                              </Link>
                            </SidebarMenuSubButton>
                          </SidebarMenuSubItem>
                        ))}
                      </SidebarMenuSub>
                    </CollapsibleContent>
                  </>
                ) : null}
              </SidebarMenuItem>
            </Collapsible>
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <UserMenu user={user} />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
