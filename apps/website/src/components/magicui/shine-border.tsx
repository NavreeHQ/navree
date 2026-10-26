import type { CSSProperties, HTMLAttributes } from "react";

import { cn } from "@/lib/utils";

interface ShineBorderProps extends HTMLAttributes<HTMLDivElement> {
  /** Border width, px. */
  borderWidth?: number;
  /** Seconds per sweep. */
  duration?: number;
  /** One color or several; Navree green by default. */
  shineColor?: string | string[];
}

/**
 * A slow sheen that sweeps around the border of its parent (which needs
 * `relative` and a radius). Use it on one element per page at most: the
 * install command, the primary card. Still under prefers-reduced-motion.
 */
export function ShineBorder({
  borderWidth = 1,
  duration = 14,
  shineColor = ["rgb(61 255 143 / 0.9)", "rgb(61 255 143 / 0.15)"],
  className,
  style,
  ...props
}: ShineBorderProps) {
  return (
    <div
      aria-hidden
      style={
        {
          "--border-width": `${borderWidth}px`,
          "--duration": `${duration}s`,
          backgroundImage: `radial-gradient(transparent,transparent, ${Array.isArray(shineColor) ? shineColor.join(",") : shineColor},transparent,transparent)`,
          backgroundSize: "300% 300%",
          mask: "linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0)",
          WebkitMask: "linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0)",
          WebkitMaskComposite: "xor",
          maskComposite: "exclude",
          padding: "var(--border-width)",
          ...style,
        } as CSSProperties
      }
      className={cn(
        "pointer-events-none absolute inset-0 size-full rounded-[inherit] will-change-[background-position] motion-safe:animate-shine",
        className,
      )}
      {...props}
    />
  );
}
