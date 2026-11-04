"use client";

import { ChevronDownIcon } from "lucide-react";
import { type ReactNode, useState } from "react";

import { cn } from "@/lib/utils";

/**
 * A group's cards. On a phone the group shows its first few, and a button
 * shows the rest (their lazy preview frames load only then); from `sm` every
 * card shows and the button is gone. Without JavaScript the gallery's
 * `<noscript>` style shows every card.
 */
export function GroupList({
  id,
  count,
  shown,
  group,
  children,
}: {
  id: string;
  count: number;
  /** How many cards show on a phone before "Show all". */
  shown: number;
  group: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="lg:col-span-9">
      <ul id={id} data-open={open || undefined} className="group/list grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {children}
      </ul>
      {count > shown ? (
        <button
          type="button"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => setOpen((was) => !was)}
          className="gallery-toggle mt-3 inline-flex h-10 w-full items-center justify-center gap-2 rounded-full border border-line-2 bg-ink-1 text-sm font-medium text-fg-1 transition-colors hover:border-fg-3 hover:text-fg sm:hidden"
        >
          {open ? "Show fewer" : `Show all ${count} in ${group}`}
          <ChevronDownIcon aria-hidden className={cn("size-4 transition-transform duration-200", open && "rotate-180")} />
        </button>
      ) : null}
    </div>
  );
}
