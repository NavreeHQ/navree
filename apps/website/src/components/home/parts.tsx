import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** One homepage section: a hairline on top, the site container, the section rhythm. */
export function HomeSection({
  id,
  children,
  className,
  inner,
}: {
  /** The id of the section's heading. */
  id: string;
  children: ReactNode;
  className?: string;
  /** Classes for the inner container. */
  inner?: string;
}) {
  return (
    <section aria-labelledby={id} className={cn("relative border-t border-line", className)}>
      <div className={cn("container-site section-y relative", inner)}>{children}</div>
    </section>
  );
}

/**
 * Eyebrow, heading and lead: the top of every section. Sections whose
 * content runs full width below use `split`: the heading on the left and the
 * lead (and an optional link) on the right, bottom-aligned, the same header
 * the architecture, ecosystem and performance sections draw.
 */
export function SectionHeading({
  id,
  eyebrow,
  title,
  lead,
  action,
  split,
  className,
}: {
  id: string;
  eyebrow: string;
  title: ReactNode;
  lead?: ReactNode;
  /** A link under the lead (split layout). */
  action?: ReactNode;
  split?: boolean;
  className?: string;
}) {
  if (split) {
    return (
      <div className={cn("grid gap-6 lg:grid-cols-12 lg:items-end lg:gap-12", className)}>
        <div className="lg:col-span-7">
          <p className="eyebrow">{eyebrow}</p>
          <h2 id={id} className="mt-5 text-display-lg font-semibold text-balance text-fg">
            {title}
          </h2>
        </div>
        {lead || action ? (
          <div className="lg:col-span-5 lg:pb-1">
            {lead ? <div className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">{lead}</div> : null}
            {action ? <div className="mt-5">{action}</div> : null}
          </div>
        ) : null}
      </div>
    );
  }
  return (
    <div className={cn("max-w-3xl", className)}>
      <p className="eyebrow">{eyebrow}</p>
      <h2 id={id} className="mt-5 text-display-lg font-semibold text-balance text-fg">
        {title}
      </h2>
      {lead ? <div className="mt-5 max-w-2xl text-lg leading-relaxed text-pretty text-fg-2">{lead}</div> : null}
    </div>
  );
}

/** A quiet text link with a trailing arrow that nudges on hover. */
export function ArrowLink({ href, children, className }: { href: string; children: ReactNode; className?: string }) {
  return (
    <Link
      href={href}
      className={cn(
        "group inline-flex w-fit items-center gap-1.5 rounded-md text-[0.9375rem] font-medium text-fg-1 transition-colors hover:text-accent",
        className,
      )}
    >
      {children}
      <ArrowRightIcon
        aria-hidden
        className="size-4 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
      />
    </Link>
  );
}

/** The green call to action, with its arrow in a nested circle (as in the header). */
export function PrimaryCta({ href, children, className }: { href: string; children: ReactNode; className?: string }) {
  return (
    <Button asChild size="lg" className={cn("pr-1.5 pl-6", className)}>
      <Link href={href} className="group">
        {children}
        <span
          aria-hidden
          className="inline-flex size-9 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
        >
          <ArrowRightIcon className="size-4" />
        </span>
      </Link>
    </Button>
  );
}

/** The quiet partner of `PrimaryCta`. */
export function SecondaryCta({ href, children, className }: { href: string; children: ReactNode; className?: string }) {
  return (
    <Button asChild size="lg" variant="outline" className={cn("px-6", className)}>
      <Link href={href}>{children}</Link>
    </Button>
  );
}
