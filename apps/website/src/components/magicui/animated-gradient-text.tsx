import type { ComponentPropsWithoutRef, CSSProperties } from "react";

import { cn } from "@/lib/utils";

export interface AnimatedGradientTextProps extends ComponentPropsWithoutRef<"span"> {
  speed?: number;
  colorFrom?: string;
  colorTo?: string;
}

/**
 * Text filled with a slowly moving gradient, green to pale mint by default.
 * For a word or two at most (an eyebrow, one word of a heading). Still under
 * prefers-reduced-motion.
 */
export function AnimatedGradientText({
  children,
  className,
  speed = 1,
  colorFrom = "#3dff8f",
  colorTo = "#d6ffe7",
  ...props
}: AnimatedGradientTextProps) {
  return (
    <span
      style={
        {
          "--bg-size": `${speed * 300}%`,
          "--color-from": colorFrom,
          "--color-to": colorTo,
        } as CSSProperties
      }
      className={cn(
        "inline bg-linear-to-r from-(--color-from) via-(--color-to) to-(--color-from) bg-size-[var(--bg-size)_100%] bg-clip-text text-transparent motion-safe:animate-gradient",
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}
