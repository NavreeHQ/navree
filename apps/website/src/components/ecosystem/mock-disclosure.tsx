"use client";

import { useId, useState, type ReactNode } from "react";

import { ChevronIcon } from "@/components/icons";
import { cn } from "@/lib/utils";

/**
 * A product mock, behind a button on phones and always shown from `sm` up.
 * The mocks are tall pictures (a phone scrolled past six of them is 28,000 px
 * long), so a phone gets the tool's words first and opens the picture when it
 * asks. Closed, the mock is `display: none`: it is not painted, and a mock
 * that is not on screen does not animate.
 */
export function MockDisclosure({ name, children }: { name: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <div>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((value) => !value)}
        className="inline-flex h-11 w-full items-center justify-between gap-3 rounded-xl border border-line-2 bg-ink-1 px-4 text-[0.9375rem] font-medium text-fg transition-colors hover:border-fg-3 hover:bg-ink-2 sm:hidden"
      >
        {open ? `Hide the ${name} screen` : `Show the ${name} screen`}
        <ChevronIcon aria-hidden className={cn("size-4 shrink-0 text-fg-3 transition-transform duration-200", open && "rotate-90")} />
      </button>
      <div id={id} className={cn("max-sm:mt-4", !open && "max-sm:hidden")}>
        {children}
      </div>
    </div>
  );
}
