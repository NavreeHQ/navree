"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { useWorkspaceSlug, useWorkspaces } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

/** The settings screens: the workspace's, then the signed-in person's own. */
export default function SettingsLayout({ children }: { children: ReactNode }) {
  const slug = useWorkspaceSlug();
  const pathname = usePathname();
  const current = useWorkspaces().data?.find((workspace) => workspace.slug === slug);
  const manager = current?.role === "owner" || current?.role === "admin";

  const workspace = [
    { href: workspacePath(slug, "/settings"), label: "General" },
    { href: workspacePath(slug, "/settings/members"), label: "Members" },
    ...(manager ? [{ href: workspacePath(slug, "/settings/billing"), label: "Billing" }] : []),
    { href: workspacePath(slug, "/settings/notifications"), label: "Notifications" },
  ];
  const account = [
    { href: workspacePath(slug, "/settings/profile"), label: "Profile" },
    { href: workspacePath(slug, "/settings/security"), label: "Password and security" },
  ];

  const link = ({ href, label }: { href: string; label: string }) => {
    const active = href.endsWith("/settings") ? pathname === href : pathname.startsWith(href);
    return (
      <Link
        key={href}
        href={href}
        aria-current={active ? "page" : undefined}
        className={cn(
          "rounded-md px-2 py-1.5 text-sm text-fg-2 hover:bg-ink-2 hover:text-fg",
          active && "bg-ink-3 text-fg",
        )}
      >
        {label}
      </Link>
    );
  };

  return (
    <div className="flex flex-col gap-8 lg:flex-row">
      <nav className="flex shrink-0 flex-col gap-4 lg:w-52" aria-label="Settings">
        <div className="flex flex-col gap-1">
          <p className="eyebrow px-2">Workspace</p>
          {workspace.map(link)}
        </div>
        <div className="flex flex-col gap-1">
          <p className="eyebrow px-2">Your account</p>
          {account.map(link)}
        </div>
      </nav>
      <div className="flex min-w-0 max-w-3xl flex-1 flex-col gap-6">{children}</div>
    </div>
  );
}
