import { LockIcon } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/**
 * The double-bezel window every product mock sits in: a browser (with an
 * address bar) or a terminal (with a title). The mock is a picture, so its
 * contents are hidden from assistive technology and `label` says in words
 * what it shows.
 */
export function MockFrame({
  url,
  title,
  label,
  status,
  children,
  className,
  bodyClassName,
}: {
  /** Shown in an address bar: a browser window. */
  url?: string;
  /** Shown in the title bar: a terminal or app window. */
  title?: string;
  /** What the mock shows, for screen readers. */
  label: string;
  /** Something small on the right of the title bar, such as a live indicator. */
  status?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <figure className={cn("bezel relative m-0 min-w-0", className)}>
      <figcaption className="sr-only">{label}</figcaption>
      <div aria-hidden className="bezel-core flex h-full min-w-0 flex-col">
        <div className="grid h-10 shrink-0 grid-cols-[minmax(max-content,1fr)_minmax(0,24rem)_minmax(max-content,1fr)] items-center gap-3 border-b border-line bg-ink-1 px-3.5">
          <div className="flex gap-1.5">
            <span className="size-2.5 rounded-full bg-line-2" />
            <span className="size-2.5 rounded-full bg-line-2" />
            <span className="size-2.5 rounded-full bg-line-2" />
          </div>
          {url ? (
            <div className="flex h-6 min-w-0 items-center justify-center gap-1.5 rounded-md border border-line bg-ink-0/70 px-2.5 font-mono text-xs text-fg-3">
              <LockIcon className="size-3 shrink-0" strokeWidth={1.75} />
              <span className="truncate">{url}</span>
            </div>
          ) : (
            <div className="min-w-0 truncate text-center font-mono text-xs text-fg-3">{title}</div>
          )}
          <div className="flex justify-end">{status}</div>
        </div>
        <div className={cn("relative min-h-0 min-w-0 flex-1", bodyClassName)}>{children}</div>
      </div>
    </figure>
  );
}
