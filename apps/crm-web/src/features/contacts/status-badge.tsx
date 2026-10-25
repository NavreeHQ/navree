import type { ContactStatus } from "@/lib/api/types";
import { cn } from "@/lib/utils";

const STYLE: Record<ContactStatus, { label: string; dot: string }> = {
  lead: { label: "Lead", dot: "bg-muted-foreground/50" },
  prospect: { label: "Prospect", dot: "bg-primary" },
  customer: { label: "Customer", dot: "bg-success" },
  churned: { label: "Churned", dot: "bg-destructive/70" },
};

/** Lifecycle status: neutral text, a small dot carries the state (always labelled). */
export function ContactStatusBadge({ status, className }: { status: ContactStatus; className?: string }) {
  const s = STYLE[status];
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center gap-1.5 rounded-[4px] border bg-card px-1.5 text-xs font-medium whitespace-nowrap",
        className,
      )}
    >
      <span aria-hidden="true" className={cn("size-1.5 rounded-full", s.dot)} />
      {s.label}
    </span>
  );
}
