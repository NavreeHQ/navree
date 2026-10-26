import type { ContactStatus } from "@/lib/api/types";
import { cn } from "@/lib/utils";

const STYLE: Record<ContactStatus, { label: string; dot: string }> = {
  lead: { label: "Lead", dot: "bg-fg-3" },
  prospect: { label: "Prospect", dot: "bg-stage-proposal" },
  customer: { label: "Customer", dot: "bg-accent shadow-[0_0_6px_rgb(61_255_143/0.7)]" },
  churned: { label: "Churned", dot: "bg-danger" },
};

/** Lifecycle status: neutral text, a small dot carries the state (always labelled). */
export function ContactStatusBadge({ status, className }: { status: ContactStatus; className?: string }) {
  const s = STYLE[status];
  return (
    <span
      className={cn(
        "inline-flex h-[1.375rem] items-center gap-1.5 rounded-full border border-line-2 bg-ink-2 px-2 text-xs font-medium whitespace-nowrap text-fg-1",
        className,
      )}
    >
      <span aria-hidden="true" className={cn("size-1.5 rounded-full", s.dot)} />
      {s.label}
    </span>
  );
}
