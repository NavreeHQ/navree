"use client";

import { useAbility } from "@navree/next/abilities";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useCurrentUser } from "@/features/auth/queries";
import { cn } from "@/lib/utils";

export function SettingsNav() {
  const pathname = usePathname();
  const ability = useAbility();
  const me = useCurrentUser();
  const items = [
    { href: "/settings", label: "General", show: true },
    { href: "/settings/users", label: "Users and roles", show: ability.can("create", "User") },
    { href: "/settings/audit", label: "Audit log", show: Boolean(me.data?.roles.includes("admin")) },
  ].filter((i) => i.show);

  return (
    <nav aria-label="Settings sections" className="flex scroll-thin gap-5 overflow-x-auto border-b">
      {items.map((item) => {
        const active = pathname === item.href;
        return (
          <Link
            key={item.href}
            href={item.href}
            aria-current={active ? "page" : undefined}
            className={cn(
              "-mb-px border-b-2 px-1 py-2 text-sm whitespace-nowrap transition-colors",
              active ? "border-fg font-medium text-fg" : "border-transparent text-fg-2 hover:text-fg",
            )}
          >
            {item.label}
          </Link>
        );
      })}
    </nav>
  );
}
