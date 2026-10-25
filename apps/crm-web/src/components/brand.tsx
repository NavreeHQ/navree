import { cn } from "@/lib/utils";

/** The Navree mark: a hull in side profile with its navree fin beneath the waterline. */
export function NavreeMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" className={cn("size-5", className)}>
      <path
        d="M2.5 9.5h19l-2.4 4.1a2 2 0 0 1-1.73 1H6.63a2 2 0 0 1-1.73-1L2.5 9.5Z"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
      <path d="M10.2 14.6 11 20.5h2l.8-5.9" fill="currentColor" />
    </svg>
  );
}

export function NavreeWordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-2", className)}>
      <NavreeMark className="text-sidebar-primary" />
      <span className="text-[17px] font-semibold tracking-tight font-condensed">Navree</span>
      <span className="text-xs font-medium text-sidebar-foreground/60">CRM</span>
    </span>
  );
}
