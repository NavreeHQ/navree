import { ArrowRightIcon, ArrowUpRightIcon } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** The green call to action, with its arrow in a nested circle. */
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

export function SecondaryCta({ href, children, className }: { href: string; children: ReactNode; className?: string }) {
  return (
    <Button asChild size="lg" variant="outline" className={cn("px-6", className)}>
      <Link href={href}>{children}</Link>
    </Button>
  );
}

/** A quiet text link with an arrow that nudges on hover; `external` for files and other sites. */
export function ArrowLink({
  href,
  children,
  external = false,
  className,
}: {
  href: string;
  children: ReactNode;
  external?: boolean;
  className?: string;
}) {
  const classes = cn(
    "group inline-flex w-fit items-center gap-1.5 rounded-md text-[0.9375rem] font-medium text-fg-1 transition-colors hover:text-accent",
    className,
  );
  const Icon = external ? ArrowUpRightIcon : ArrowRightIcon;
  const icon = (
    <Icon
      aria-hidden
      className={cn(
        "size-4 transition-transform duration-300 ease-(--ease-out-quint)",
        external ? "group-hover:translate-x-0.5 group-hover:-translate-y-0.5" : "group-hover:translate-x-0.5",
      )}
    />
  );
  if (external || !href.startsWith("/") || /\.(md|txt|json)$/.test(href)) {
    return (
      <a href={href} className={classes}>
        {children}
        {icon}
      </a>
    );
  }
  return (
    <Link href={href} className={classes}>
      {children}
      {icon}
    </Link>
  );
}

/** Eyebrow, heading and lead at the top of each half of the page. */
export function HalfHeading({
  id,
  index,
  eyebrow,
  title,
  lead,
}: {
  id: string;
  index: string;
  eyebrow: string;
  title: ReactNode;
  lead: ReactNode;
}) {
  return (
    <div className="grid grid-cols-1 gap-x-12 gap-y-6 lg:grid-cols-12">
      <div className="lg:col-span-7">
        <p className="eyebrow">
          <span className="text-fg-3">{index}</span> {eyebrow}
        </p>
        <h2 id={id} className="mt-5 text-display-lg font-semibold text-balance text-fg">
          {title}
        </h2>
      </div>
      <div className="max-w-2xl text-lg leading-relaxed text-pretty text-fg-2 lg:col-span-5 lg:self-end">{lead}</div>
    </div>
  );
}

/** The small mono label and display heading of one block inside a half. */
export function BlockHeading({ label, title, id }: { label: string; title: ReactNode; id?: string }) {
  return (
    <>
      <p className="font-mono text-[0.8125rem] text-accent">{label}</p>
      <h3 id={id} className="mt-3 font-display text-display-md font-semibold text-balance text-fg">
        {title}
      </h3>
    </>
  );
}

/** A file or window mock: a title bar and a mono body. */
export function WindowFrame({
  title,
  aside,
  children,
  className,
  bodyClassName,
}: {
  title: ReactNode;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <div className={cn("bezel", className)}>
      <div className="bezel-core">
        <div className="flex h-10 items-center justify-between gap-3 border-b border-line px-4">
          <span className="min-w-0 truncate font-mono text-xs text-fg-3">{title}</span>
          {aside}
        </div>
        <div className={bodyClassName}>{children}</div>
      </div>
    </div>
  );
}
