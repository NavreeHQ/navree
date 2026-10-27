"use client";

import { BellIcon, CreditCardIcon, FolderIcon, GaugeIcon, SettingsIcon, UsersIcon } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { UserMenu } from "@/components/user-menu";
import { WorkspaceSwitcher } from "@/components/workspace-switcher";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar";
import type { User, Workspace } from "@/lib/api/types";
import { workspacePath } from "@/lib/workspace";

/** Whether `href` is the current page or one of its children. */
export function isActive(pathname: string, href: string, exact = false): boolean {
  return exact ? pathname === href : pathname === href || pathname.startsWith(`${href}/`);
}

/**
 * The app sidebar (shadcn/ui's `sidebar-07` layout): the workspace switcher, the product and
 * workspace groups, the user menu in the footer. Collapses to icons (Ctrl/Cmd+B, the rail, the
 * top bar trigger) and becomes a sheet on phones.
 */
export function AppSidebar({ user, current, workspaces }: { user: User; current: Workspace; workspaces: Workspace[] }) {
  const pathname = usePathname();
  const { setOpenMobile } = useSidebar();
  const path = (rest = "") => workspacePath(current.slug, rest);

  const product = [
    { href: path(), label: "Dashboard", icon: GaugeIcon, exact: true },
    { href: path("/projects"), label: "Projects", icon: FolderIcon, exact: false },
  ];
  const workspace = [
    { href: path("/settings"), label: "Settings", icon: SettingsIcon, exact: true, show: true },
    { href: path("/settings/members"), label: "Members", icon: UsersIcon, exact: false, show: true },
    {
      href: path("/settings/billing"),
      label: "Billing",
      icon: CreditCardIcon,
      exact: false,
      // The billing routes need `billing.manage`: owners and admins in config/app.toml.
      show: current.role === "owner" || current.role === "admin",
    },
    { href: path("/settings/notifications"), label: "Notifications", icon: BellIcon, exact: false, show: true },
  ];

  const item = ({ href, label, icon: Icon, exact }: (typeof product)[number]) => {
    const active = isActive(pathname, href, exact);
    return (
      <SidebarMenuItem key={href}>
        <SidebarMenuButton asChild isActive={active} tooltip={label}>
          <Link href={href} onClick={() => setOpenMobile(false)} aria-current={active ? "page" : undefined}>
            <Icon />
            <span>{label}</span>
          </Link>
        </SidebarMenuButton>
      </SidebarMenuItem>
    );
  };

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <WorkspaceSwitcher current={current} workspaces={workspaces} />
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Product</SidebarGroupLabel>
          <SidebarMenu>{product.map(item)}</SidebarMenu>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Workspace</SidebarGroupLabel>
          <SidebarMenu>{workspace.filter((entry) => entry.show).map(item)}</SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <UserMenu user={user} workspace={current.slug} />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
