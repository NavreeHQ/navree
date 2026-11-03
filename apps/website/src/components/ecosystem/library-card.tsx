import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

import { PlannedBadge } from "@/components/site/planned-badge";
import type { EcosystemEntry } from "@/content/ecosystem";
import { cn } from "@/lib/utils";
import { site } from "@site";

import { CodeLine } from "./code-line";
import { hasMark, ProductMark } from "./marks";
import { SpotlightCard } from "./spotlight-card";

/**
 * A package's card: its name, what it does, the one line of code that shows
 * its API, and a link to its docs. The whole card is the link. `children`
 * adds a small visual between the words and the code.
 */
export function LibraryCard({ entry, children, className }: { entry: EcosystemEntry; children?: ReactNode; className?: string }) {
  const planned = site.showPlannedBadges && entry.status === "planned";
  return (
    <SpotlightCard className={cn("h-full rounded-2xl", className)}>
      {/* Not prefetched: the ecosystem page has a card per package, and scrolling past them would fetch every docs page. */}
      <Link
        href={entry.href}
        prefetch={false}
        className="group flex h-full flex-col rounded-2xl p-5 focus-visible:outline-offset-[-2px] sm:p-6"
        aria-labelledby={`${entry.slug}-card-title`}
        aria-describedby={`${entry.slug}-card-desc`}
      >
        <div className="flex items-start gap-3">
          {hasMark(entry.slug) ? <ProductMark slug={entry.slug} size="sm" className="-mt-0.5" /> : null}
          <h3 id={`${entry.slug}-card-title`} className="min-w-0 flex-1 font-sans text-[1.0625rem] leading-snug font-semibold tracking-[-0.01em] text-fg">{entry.name}</h3>
          {planned ? <PlannedBadge /> : null}
          <span className="hidden shrink-0 rounded-md border border-line px-1.5 font-mono text-xs leading-5 text-fg-3 sm:inline">{entry.crate}</span>
        </div>
        <p id={`${entry.slug}-card-desc`} className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
          {entry.summary}
        </p>
        {children ? <div className="mt-5">{children}</div> : null}
        <div className="mt-auto pt-5">
          {entry.signature ? (
            <div className="overflow-hidden rounded-lg border border-line bg-ink-0/60 px-3 py-1.5">
              <CodeLine code={entry.signature} className="block truncate" />
            </div>
          ) : null}
          <span className="mt-4 inline-flex items-center gap-1.5 text-sm font-medium text-fg-1 transition-colors duration-200 group-hover:text-accent">
            {entry.name} docs
            <ArrowRightIcon aria-hidden className="size-3.5 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5" />
          </span>
        </div>
      </Link>
    </SpotlightCard>
  );
}
