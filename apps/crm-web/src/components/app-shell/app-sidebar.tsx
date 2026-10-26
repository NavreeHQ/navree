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
import { NavreeWordmark } from "@/components/brand";
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

/** The current page carries the green running light: a rule on the left edge and a green icon. */
const ACTIVE_MARK =
  "text-fg-2 [&>svg]:text-fg-3 data-[active=true]:shadow-[inset_2px_0_0_var(--color-accent)] data-[active=true]:[&>svg]:text-accent";

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
      <SidebarHeader className="h-(--header-h) justify-center border-b border-sidebar-border px-3 group-data-[collapsible=icon]:px-2">
        <Link href="/" onClick={closeMobile} aria-label="Navree CRM home" className="flex items-center rounded-md px-1">
          <NavreeWordmark className="group-data-[collapsible=icon]:[&>span]:hidden" />
        </Link>
      </SidebarHeader>
      <SidebarContent className="scroll-thin pt-2">
        <SidebarGroup>
          <SidebarGroupLabel>Sales</SidebarGroupLabel>
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
          <p className="flex gap-2 rounded-lg border border-line-2 bg-ink-2/60 px-3 py-2.5 text-xs leading-snug text-fg-2">
            <span aria-hidden="true" className="mt-1 size-1.5 shrink-0 rounded-full bg-amber" />
            Demo data from the mock API. Changes last until you close this tab.
          </p>
        </SidebarFooter>
      ) : null}
    </Sidebar>
  );
}
