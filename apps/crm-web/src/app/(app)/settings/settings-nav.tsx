"use client";

import { useAbility } from "@casl/react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useCurrentUser } from "@/features/auth/queries";
import type { AppAbility } from "@/lib/casl/ability";
import { cn } from "@/lib/utils";

export function SettingsNav() {
  const pathname = usePathname();
  const ability = useAbility<AppAbility>();
  const me = useCurrentUser();
  const items = [
    { href: "/settings", label: "General", show: true },
    { href: "/settings/users", label: "Users and roles", show: ability.can("create", "User") },
    { href: "/settings/audit", label: "Audit log", show: Boolean(me.data?.roles.includes("admin")) },
  ].filter((i) => i.show);

  return (
    <nav aria-label="Settings sections" className="flex gap-1 border-b">
      {items.map((item) => {
        const active = pathname === item.href;
        return (
          <Link
            key={item.href}
            href={item.href}
            aria-current={active ? "page" : undefined}
            className={cn(
              "-mb-px border-b-2 px-3 py-2 text-sm transition-colors",
              active
                ? "border-primary font-medium text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground",
            )}
          >
            {item.label}
          </Link>
        );
      })}
    </nav>
  );
}
