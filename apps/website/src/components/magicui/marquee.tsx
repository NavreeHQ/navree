import type { ComponentPropsWithoutRef, ReactNode } from "react";

import { cn } from "@/lib/utils";

interface MarqueeProps extends ComponentPropsWithoutRef<"div"> {
  reverse?: boolean;
  pauseOnHover?: boolean;
  children: ReactNode;
  vertical?: boolean;
  /** How many copies of the content run side by side (default 4). */
  repeat?: number;
}

/**
 * An endless row (or column) of items. Set the speed and gap with
 * `[--duration:40s] [--gap:1rem]` in `className`. Only the first copy is
 * exposed to assistive technology. Under prefers-reduced-motion it stands
 * still (the global reduced-motion rule), and it pauses on hover and focus.
 * Fade the edges with `mask-x-from-90%` or a gradient overlay.
 */
export function Marquee({ className, reverse = false, pauseOnHover = true, children, vertical = false, repeat = 4, ...props }: MarqueeProps) {
  return (
    <div
      {...props}
      className={cn(
        "group flex gap-(--gap) overflow-hidden [--duration:40s] [--gap:1rem]",
        vertical ? "flex-col" : "flex-row",
        className,
      )}
    >
      {Array.from({ length: repeat }, (_, i) => (
        <div
          key={i}
          aria-hidden={i > 0 ? true : undefined}
          className={cn(
            "flex shrink-0 justify-around gap-(--gap) motion-reduce:animate-none",
            vertical ? "animate-marquee-vertical flex-col" : "animate-marquee flex-row",
            pauseOnHover && "group-focus-within:[animation-play-state:paused] group-hover:[animation-play-state:paused]",
            reverse && "[animation-direction:reverse]",
          )}
        >
          {children}
        </div>
      ))}
    </div>
  );
}
