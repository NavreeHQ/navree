import { useId } from "react";

import { cn } from "@/lib/utils";

import type { DotPatternProps } from "./dot-pattern-glow";

/**
 * A dot grid background. Absolutely positioned: put it in a `relative`
 * parent and fade it with a mask, e.g.
 * `[mask-image:radial-gradient(ellipse_at_center,black,transparent_70%)]`.
 * Dots take the text color (`text-line-2` by default).
 *
 * Static SVG rendered on the server, with no JavaScript in the browser. For
 * a few dots pulsing green at random, use `DotPatternGlow` (`./dot-pattern-glow`,
 * a client component) with `glow`.
 */
export function DotPattern({
  width = 16,
  height = 16,
  x = 0,
  y = 0,
  cx = 1,
  cy = 1,
  cr = 1,
  className,
  ...props
}: Omit<DotPatternProps, "glow" | "glowCount">) {
  const id = useId();
  return (
    <svg aria-hidden="true" className={cn("pointer-events-none absolute inset-0 size-full text-line-2", className)} {...props}>
      <defs>
        <pattern id={`${id}-dots`} width={width} height={height} patternUnits="userSpaceOnUse" x={x} y={y}>
          <circle cx={cx} cy={cy} r={cr} fill="currentColor" />
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${id}-dots)`} />
    </svg>
  );
}
