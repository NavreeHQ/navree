import { cn } from "@/lib/utils";

/**
 * The "In development" badge for planned pages and catalogue entries.
 * Callers decide whether to show it:
 *
 *   {site.showPlannedBadges && entry.status === "planned" ? <PlannedBadge /> : null}
 */
export function PlannedBadge({ className, compact = false }: { className?: string; compact?: boolean }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-full border border-amber/30 bg-amber/10 font-medium whitespace-nowrap text-amber",
        compact ? "px-1.5 py-px text-[0.625rem] leading-4" : "px-2.5 py-0.5 text-xs",
        className,
      )}
      title="Planned: documented here, being built now"
    >
      <span aria-hidden className="size-1.5 rounded-full bg-amber" />
      In development
    </span>
  );
}
