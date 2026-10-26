"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/** True while `pathname` is `match` or inside it. */
export function isCurrent(pathname: string, match: string) {
  return pathname === match || pathname.startsWith(`${match}/`);
}

/** A header link that marks itself current while you are in its section. */
export function NavLink({ href, match, children }: { href: string; match: string; children: ReactNode }) {
  const pathname = usePathname() ?? "";
  const current = isCurrent(pathname, match);
  return (
    <Link
      href={href}
      aria-current={current ? "page" : undefined}
      className={cn(
        "relative rounded-full px-3 py-1.5 text-[0.875rem] transition-colors duration-200",
        current ? "text-fg" : "text-fg-2 hover:bg-ink-2 hover:text-fg",
      )}
    >
      {children}
      {current ? (
        <span
          aria-hidden
          className="absolute inset-x-3 -bottom-[calc((var(--header-h)-2rem)/2+1px)] h-px bg-accent shadow-[0_0_8px_rgb(61_255_143/0.6)]"
        />
      ) : null}
    </Link>
  );
}
