import type { ReactNode } from "react";
import { PageActions } from "@/components/app-shell/shell-slots";
import { cn } from "@/lib/utils";

/**
 * The title block of a screen: an optional small label, the H1, a line of
 * context. `actions` (the "New …" button, Export) go to the top bar from `md` up
 * and stay here on phones; `aside` (filters that shape the page) always stays here.
 */
export function PageHeader({
  title,
  description,
  actions,
  aside,
  children,
  eyebrow,
  className,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  aside?: ReactNode;
  children?: ReactNode;
  eyebrow?: ReactNode;
  className?: string;
}) {
  return (
    <header className={cn("flex flex-col gap-4 pb-5", className)}>
      <div className="flex flex-wrap items-end justify-between gap-x-6 gap-y-3">
        <div className="flex min-w-0 flex-col gap-1">
          {eyebrow ? <p className="eyebrow">{eyebrow}</p> : null}
          <h1 className="text-title font-semibold text-fg">{title}</h1>
          {description ? <p className="text-sm text-pretty text-fg-2">{description}</p> : null}
        </div>
        {aside || actions ? (
          <div className="flex flex-wrap items-center gap-2 empty:hidden">
            {aside}
            {actions ? <PageActions className="contents">{actions}</PageActions> : null}
          </div>
        ) : null}
      </div>
      {children}
    </header>
  );
}

/** A titled section inside a page (panels on detail pages and the dashboard). */
export function Panel({
  title,
  actions,
  children,
  className,
  bodyClassName,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section className={cn("surface min-w-0 text-card-foreground", className)}>
      {title || actions ? (
        <div className="flex min-h-11 items-center justify-between gap-3 border-b px-4 py-1.5">
          {title ? <h2 className="text-sm font-semibold text-fg">{title}</h2> : <span />}
          {actions ? <div className="flex items-center gap-1.5">{actions}</div> : null}
        </div>
      ) : null}
      <div className={cn("p-4", bodyClassName)}>{children}</div>
    </section>
  );
}

/** Definition list row for record details. */
export function DetailRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[7rem_minmax(0,1fr)] items-baseline gap-3 py-1.5 text-sm">
      <dt className="text-fg-3">{label}</dt>
      <dd className="min-w-0 break-words text-fg-1">{children ?? <span className="text-fg-3">None</span>}</dd>
    </div>
  );
}

/** A row of key figures (dashboard, record pages): label above, the number in the display face. */
export function FigureGrid({
  children,
  columns,
  className,
  label,
}: {
  children: ReactNode;
  columns: 3 | 4;
  className?: string;
  label?: string;
}) {
  return (
    <dl
      aria-label={label}
      className={cn(
        "surface grid overflow-hidden [&>*]:border-line",
        columns === 3
          ? "grid-cols-3 [&>*:not(:first-child)]:border-l"
          : "grid-cols-2 lg:grid-cols-4 lg:[&>*:not(:first-child)]:border-l [&>*:nth-child(2n)]:border-l [&>*:nth-child(n+3)]:border-t lg:[&>*:nth-child(n+3)]:border-t-0",
        className,
      )}
    >
      {children}
    </dl>
  );
}

export function Figure({
  label,
  children,
  sub,
  highlight,
}: {
  label: ReactNode;
  children: ReactNode;
  sub?: ReactNode;
  highlight?: boolean;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5 px-4 py-3.5">
      <dt className="text-xs text-fg-3">{label}</dt>
      <dd className={cn("font-display text-display-sm font-semibold figures", highlight ? "text-accent" : "text-fg")}>
        {children}
      </dd>
      {sub ? <dd className="text-xs text-fg-3">{sub}</dd> : null}
    </div>
  );
}
