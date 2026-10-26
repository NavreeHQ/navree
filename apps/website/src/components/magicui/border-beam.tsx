"use client";

import { motion, useReducedMotion, type MotionStyle, type Transition } from "motion/react";
import type { CSSProperties } from "react";

import { cn } from "@/lib/utils";

interface BorderBeamProps {
  /** The length of the beam in px. */
  size?: number;
  /** Seconds per lap. */
  duration?: number;
  delay?: number;
  colorFrom?: string;
  colorTo?: string;
  transition?: Transition;
  className?: string;
  style?: CSSProperties;
  reverse?: boolean;
  /** The starting position along the border, 0-100. */
  initialOffset?: number;
  borderWidth?: number;
}

/**
 * A short streak of green light running around the border of its parent.
 * The parent needs `relative`, a radius and `overflow-hidden`. It is not
 * rendered under prefers-reduced-motion.
 */
export const BorderBeam = ({
  className,
  size = 80,
  delay = 0,
  duration = 8,
  colorFrom = "#3dff8f",
  colorTo = "rgb(61 255 143 / 0.35)",
  transition,
  style,
  reverse = false,
  initialOffset = 0,
  borderWidth = 1,
}: BorderBeamProps) => {
  const reduced = useReducedMotion();
  if (reduced) return null;

  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-0 rounded-[inherit] border-(length:--border-beam-width) border-transparent mask-[linear-gradient(transparent,transparent),linear-gradient(#000,#000)] mask-intersect [mask-clip:padding-box,border-box]"
      style={{ "--border-beam-width": `${borderWidth}px` } as CSSProperties}
    >
      <motion.div
        className={cn("absolute aspect-square", "bg-linear-to-l from-(--color-from) via-(--color-to) to-transparent", className)}
        style={
          {
            width: size,
            offsetPath: `rect(0 auto auto 0 round ${size}px)`,
            "--color-from": colorFrom,
            "--color-to": colorTo,
            ...style,
          } as MotionStyle
        }
        initial={{ offsetDistance: `${initialOffset}%` }}
        animate={{
          offsetDistance: reverse
            ? [`${100 - initialOffset}%`, `${-initialOffset}%`]
            : [`${initialOffset}%`, `${100 + initialOffset}%`],
        }}
        transition={{ repeat: Infinity, ease: "linear", duration, delay: -delay, ...transition }}
      />
    </div>
  );
};
