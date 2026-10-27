"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Fragment } from "react";
import { CommandSearch } from "@/components/app-shell/command-search";
import { NotificationsBell } from "@/components/app-shell/notifications-bell";
import { useActionsSlotRef, useRecordTitle } from "@/components/app-shell/shell-slots";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { Skeleton } from "@/components/ui/skeleton";

type Crumb = { label: string; href?: string };

const SECTIONS: Record<string, { group: string; label: string; record?: string }> = {
  "": { group: "Sales", label: "Dashboard" },
  companies: { group: "Sales", label: "Companies", record: "Company" },
  contacts: { group: "Sales", label: "Contacts", record: "Contact" },
  deals: { group: "Sales", label: "Deals", record: "Deal" },
  tasks: { group: "Sales", label: "Tasks" },
  settings: { group: "Workspace", label: "Settings" },
};

const SETTINGS_PAGES: Record<string, string> = { users: "Users and roles", audit: "Audit log" };

/**
 * Where you are: the sidebar group, the screen, and the record or settings page.
 * `null` as the last label means a record whose name is still loading.
 */
export function breadcrumbFor(
  pathname: string,
  recordTitle: string | null,
): { group?: string; trail: (Crumb | null)[] } {
  const [first = "", second] = pathname.split("/").filter(Boolean);
  const section = SECTIONS[first];
  if (!section) return { trail: [] };
  if (!second) return { group: section.group, trail: [{ label: section.label }] };
  const parent = { label: section.label, href: `/${first}` };
  if (first === "settings")
    return { group: section.group, trail: [parent, { label: SETTINGS_PAGES[second] ?? second }] };
  return { group: section.group, trail: [parent, recordTitle ? { label: recordTitle } : null] };
}

function Trail() {
  const pathname = usePathname();
  const recordTitle = useRecordTitle();
  const { group, trail } = breadcrumbFor(pathname, recordTitle);

  return (
    <Breadcrumb className="min-w-0">
      <BreadcrumbList className="flex-nowrap gap-1.5 text-[0.8125rem] text-fg-3 sm:gap-1.5">
        {group ? (
          <>
            <BreadcrumbItem className="hidden lg:inline-flex">{group}</BreadcrumbItem>
            <BreadcrumbSeparator className="hidden text-fg-3/60 lg:block" />
          </>
        ) : null}
        {trail.map((crumb, i) => {
          const last = i === trail.length - 1;
          return (
            <Fragment key={i}>
              <BreadcrumbItem className={last ? "min-w-0" : "shrink-0"}>
                {crumb === null ? (
                  <Skeleton className="h-3.5 w-32" />
                ) : last ? (
                  <BreadcrumbPage className="truncate font-medium text-fg">{crumb.label}</BreadcrumbPage>
                ) : (
                  <BreadcrumbLink asChild className="text-fg-2 hover:text-fg">
                    <Link href={crumb.href ?? "/"}>{crumb.label}</Link>
                  </BreadcrumbLink>
                )}
              </BreadcrumbItem>
              {last ? null : <BreadcrumbSeparator className="text-fg-3/60" />}
            </Fragment>
          );
        })}
      </BreadcrumbList>
    </Breadcrumb>
  );
}

/**
 * The 48px top bar (DESIGN.md section 9): the sidebar trigger, a separator, the
 * breadcrumb; then search (Ctrl/Cmd K), notifications and the screen's actions.
 */
export function TopBar() {
  const actionsRef = useActionsSlotRef();
  return (
    <header className="sticky top-0 z-20 flex h-(--header-h) shrink-0 items-center gap-2 border-b bg-background/90 px-3 backdrop-blur-md sm:px-4 lg:px-6">
      <SidebarTrigger className="-ml-1 text-fg-2 [&_svg]:stroke-[1.75]" />
      <Separator orientation="vertical" className="mr-1 data-vertical:h-4 data-vertical:self-auto" />
      <Trail />
      <div className="ml-auto flex shrink-0 items-center gap-2">
        <CommandSearch />
        <NotificationsBell />
        <div
          ref={actionsRef}
          className="flex items-center gap-2 before:mr-1 before:h-4 before:w-px before:bg-line-2 empty:hidden max-md:hidden"
        />
      </div>
    </header>
  );
}
