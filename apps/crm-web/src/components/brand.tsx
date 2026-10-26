import { cn } from "@/lib/utils";

/**
 * The Navree mark, the same paths as navree.com: a hull in profile with its
 * sheer rising to the bow, cut by the waterline, over a swept fin. Drawn on a
 * 32-unit grid so it stays crisp at 16 px (the favicon is `src/app/icon.svg`).
 */
const MARK_PATHS = [
  "M1.5 9.5 L30.5 7 C29.6 9.4 28.6 11.3 27.4 12.7 L3.4 12.7 C2.6 11.8 2 10.8 1.5 9.5Z",
  "M4.4 14.5 L26 14.5 C24.4 16.5 22.4 17.8 19.5 18.3 L10 18.3 C7.6 17.8 5.8 16.5 4.4 14.5Z",
  "M13 18.3H19.4L16.4 26H14.4Z",
] as const;

export function NavreeMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" fill="currentColor" aria-hidden="true" className={cn("size-6 text-accent", className)}>
      {MARK_PATHS.map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  );
}

/** The mark, the wordmark and the product name, as in the sidebar. */
export function NavreeWordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-2", className)}>
      <NavreeMark className="size-7 shrink-0 drop-shadow-[0_0_10px_rgb(61_255_143/0.45)]" />
      <span className="font-display text-[1.1875rem] leading-none font-semibold tracking-[-0.03em] text-fg">
        navree
      </span>
      <span className="rounded-full border border-line-2 px-1.5 py-px font-mono text-xs leading-4 tracking-[0.08em] text-fg-2 uppercase">
        CRM
      </span>
    </span>
  );
}
