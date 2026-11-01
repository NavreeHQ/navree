import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

import { srcSet, type Shot } from "./crm-data";

/**
 * A screenshot with every exported size in `srcset`, so a 2x screen gets
 * the 2x file and a phone gets a small one. A plain `<img>`: the site is a
 * static export, where `next/image` serves one size only.
 */
export function ShotImage({
  shot,
  sizes,
  priority = false,
  className,
  alt,
}: {
  shot: Shot;
  /** The `sizes` attribute: how wide the image is drawn at each breakpoint. */
  sizes: string;
  /** The above-the-fold image: loaded eagerly and first. */
  priority?: boolean;
  className?: string;
  /** Overrides the manifest's alt text (`""` when the image is decorative). */
  alt?: string;
}) {
  return (
    // oxlint-disable-next-line next/no-img-element -- static export: srcset by hand
    <img
      src={shot.srcset.find((v) => v.width * 2 === shot.width)?.src ?? shot.src}
      srcSet={srcSet(shot)}
      sizes={sizes}
      width={shot.width}
      height={shot.height}
      alt={alt ?? shot.alt}
      loading={priority ? "eager" : "lazy"}
      fetchPriority={priority ? "high" : undefined}
      decoding="async"
      className={cn("block h-auto w-full", className)}
    />
  );
}

/** Three quiet dots, the window controls of a browser frame. */
function WindowDots() {
  return (
    <div className="flex shrink-0 gap-1.5" aria-hidden>
      <span className="size-2.5 rounded-full bg-line-2" />
      <span className="size-2.5 rounded-full bg-line-2" />
      <span className="size-2.5 rounded-full bg-line-2" />
    </div>
  );
}

/**
 * The browser window a desktop screenshot sits in: a double bezel, a
 * 40px bar with the address, then the picture edge to edge.
 */
export function BrowserFrame({
  address,
  children,
  className,
  bar,
}: {
  /** The address shown in the bar, e.g. `localhost:3000/deals`. */
  address: string;
  children: ReactNode;
  className?: string;
  /** Extra content at the right end of the bar. */
  bar?: ReactNode;
}) {
  return (
    <div className={cn("bezel", className)}>
      <div className="bezel-core">
        <div className="grid h-10 grid-cols-[minmax(max-content,1fr)_minmax(0,26rem)_minmax(max-content,1fr)] items-center gap-3 border-b border-line bg-ink-1 px-3.5">
          <WindowDots />
          <div className="flex h-6 min-w-0 items-center justify-center rounded-md border border-line bg-ink-0 px-2.5">
            <span className="truncate font-mono text-xs text-fg-3">{address}</span>
          </div>
          <div className="flex justify-end">{bar}</div>
        </div>
        <div className="relative bg-ink-0">{children}</div>
      </div>
    </div>
  );
}

/** A phone: a rounded body around a 390 by 844 screenshot. */
export function PhoneFrame({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={cn(
        "rounded-[2rem] border border-line-2 bg-ink-2 p-[5px] shadow-bezel sm:rounded-[2.5rem] sm:p-2",
        className,
      )}
    >
      <div className="overflow-hidden rounded-[1.7rem] border border-line bg-ink-0 sm:rounded-[2rem]">{children}</div>
    </div>
  );
}
