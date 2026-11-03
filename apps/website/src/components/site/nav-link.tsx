"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";
import { site } from "@site";

/** How deep `pathname` sits in one of `match`'s sections: the longest section it is in, or 0. */
function depth(pathname: string, match: readonly string[]) {
  return Math.max(0, ...match.filter((m) => pathname === m || pathname.startsWith(`${m}/`)).map((m) => m.length));
}

/**
 * True while `pathname` is in one of `match`'s sections and no other item of
 * the main navigation matches it more closely (`/docs/ui/` is "UI", not "Docs").
 */
export function isCurrent(pathname: string, match: readonly string[]) {
  const own = depth(pathname, match);
  return own > 0 && site.nav.every((item) => depth(pathname, item.match) <= own);
}

/** A header link that marks itself current while you are in its section. */
export function NavLink({ href, match, children }: { href: string; match: readonly string[]; children: ReactNode }) {
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
