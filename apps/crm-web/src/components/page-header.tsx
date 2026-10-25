import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export function PageHeader({
  title,
  description,
  actions,
  children,
  className,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <header className={cn("flex flex-col gap-4 pb-5", className)}>
      <div className="flex flex-wrap items-end justify-between gap-x-6 gap-y-3">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="text-[26px] leading-tight font-semibold tracking-[-0.015em] font-condensed">{title}</h1>
          {description ? <p className="text-sm text-muted-foreground">{description}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
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
    <section className={cn("rounded-lg border bg-card text-card-foreground", className)}>
      {title || actions ? (
        <div className="flex min-h-11 items-center justify-between gap-3 border-b px-4 py-2">
          {title ? <h2 className="text-sm font-semibold">{title}</h2> : <span />}
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
    <div className="grid grid-cols-[6.5rem_minmax(0,1fr)] items-baseline gap-3 py-1.5 text-sm">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words">{children ?? <span className="text-muted-foreground">None</span>}</dd>
    </div>
  );
}
