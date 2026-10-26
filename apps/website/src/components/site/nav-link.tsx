"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

/** A header link that marks itself current while you are in its section. */
export function NavLink({ href, match, children }: { href: string; match: string; children: ReactNode }) {
  const pathname = usePathname() ?? "";
  const current = pathname === match || pathname.startsWith(`${match}/`);
  return (
    <Link
      href={href}
      aria-current={current ? "page" : undefined}
      className={`relative rounded-md px-2 py-1.5 transition-colors sm:px-2.5 ${
        current ? "text-fg" : "text-fg-2 hover:text-fg"
      }`}
    >
      {children}
      {current ? (
        <span aria-hidden className="absolute inset-x-2.5 -bottom-3 h-px bg-accent shadow-[0_0_8px_rgb(61_255_143/0.6)]" />
      ) : null}
    </Link>
  );
}
