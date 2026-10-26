/**
 * The Navree mark: a hull in profile with its sheer rising to the bow, cut
 * by the waterline, over a swept fin. It is drawn
 * on a 32-unit grid so it stays crisp at 16 px (the favicon is the same
 * paths: `src/app/icon.svg`).
 */
export const MARK_PATHS = [
  "M1.5 9.5 L30.5 7 C29.6 9.4 28.6 11.3 27.4 12.7 L3.4 12.7 C2.6 11.8 2 10.8 1.5 9.5Z",
  "M4.4 14.5 L26 14.5 C24.4 16.5 22.4 17.8 19.5 18.3 L10 18.3 C7.6 17.8 5.8 16.5 4.4 14.5Z",
  "M13 18.3H19.4L16.4 26H14.4Z",
] as const;

export function LogoMark({ className, title }: { className?: string; title?: string }) {
  return (
    <svg
      viewBox="0 0 32 32"
      className={className}
      role={title ? "img" : undefined}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      fill="currentColor"
    >
      {MARK_PATHS.map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  );
}

/** The mark and the wordmark, as used in the header and footer. */
export function Logo({ className }: { className?: string }) {
  return (
    <span className={`inline-flex items-center gap-2 ${className ?? ""}`}>
      <LogoMark className="size-7 text-accent drop-shadow-[0_0_10px_rgb(61_255_143/0.45)]" />
      <span className="font-display text-[1.25rem] font-semibold tracking-[-0.03em] text-fg">navree</span>
    </span>
  );
}
