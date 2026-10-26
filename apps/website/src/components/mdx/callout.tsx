import type { ReactNode } from "react";

import { InfoIcon, TipIcon, WarningIcon } from "@/components/icons";

const kinds = {
  note: { Icon: InfoIcon, label: "Note", box: "border-line-2 bg-ink-1", icon: "text-fg-2" },
  tip: { Icon: TipIcon, label: "Tip", box: "border-accent/30 bg-accent/[0.045]", icon: "text-accent" },
  warning: { Icon: WarningIcon, label: "Warning", box: "border-amber/35 bg-amber/[0.06]", icon: "text-amber" },
} as const;

export type CalloutType = keyof typeof kinds;

/**
 * `<Callout type="note|tip|warning" title="Optional title">` with Markdown
 * inside. Without a title, the type's name ("Note", "Tip", "Warning") is
 * announced to screen readers only.
 */
export function Callout({ type = "note", title, children }: { type?: CalloutType; title?: string; children?: ReactNode }) {
  const kind = kinds[type] ?? kinds.note;
  const { Icon } = kind;
  return (
    <aside className={`callout my-7 flex gap-3.5 rounded-xl border px-4.5 py-4 ${kind.box}`} data-type={type}>
      <Icon className={`mt-[0.2rem] size-[1.125rem] shrink-0 ${kind.icon}`} />
      <div className="min-w-0 flex-1 text-[0.9375rem] leading-relaxed text-fg-1">
        {title ? (
          <p className="mt-0 mb-1 font-semibold text-fg">{title}</p>
        ) : (
          <span className="sr-only">{kind.label}: </span>
        )}
        <div className="callout-body">{children}</div>
      </div>
    </aside>
  );
}
