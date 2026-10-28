"use client";

import { motion, useReducedMotion } from "motion/react";
import { useEffect, useId, useRef, useState, type SVGProps } from "react";

import { cn } from "@/lib/utils";

export interface DotPatternProps extends Omit<SVGProps<SVGSVGElement>, "width" | "height" | "x" | "y"> {
  /** Horizontal spacing between dots, px. */
  width?: number;
  /** Vertical spacing between dots, px. */
  height?: number;
  x?: number;
  y?: number;
  cx?: number;
  cy?: number;
  /** Dot radius, px. */
  cr?: number;
  className?: string;
  /** A few dots pulse green at random. Off under prefers-reduced-motion. */
  glow?: boolean;
  /** How many dots pulse when `glow` is on. */
  glowCount?: number;
}

type GlowDot = { x: number; y: number; delay: number; duration: number };

/**
 * The dot grid with `glow`: a few dots pulse green at random (client side).
 * Use it through `DotPattern glow`.
 *
 * A dot grid background. Absolutely positioned: put it in a `relative`
 * parent and fade it with a mask, e.g.
 * `[mask-image:radial-gradient(ellipse_at_center,black,transparent_70%)]`.
 * Dots take the text color (`text-line-2` by default).
 */
export function DotPatternGlow({
  width = 16,
  height = 16,
  x = 0,
  y = 0,
  cx = 1,
  cy = 1,
  cr = 1,
  className,
  glow = false,
  glowCount = 24,
  ...props
}: DotPatternProps) {
  const id = useId();
  const reduced = useReducedMotion();
  const containerRef = useRef<SVGSVGElement>(null);
  const [glowDots, setGlowDots] = useState<GlowDot[]>([]);

  useEffect(() => {
    if (!glow || reduced) return;
    const place = () => {
      const el = containerRef.current;
      if (!el) return;
      const { width: w, height: h } = el.getBoundingClientRect();
      const cols = Math.max(1, Math.floor(w / width));
      const rows = Math.max(1, Math.floor(h / height));
      setGlowDots(
        Array.from({ length: glowCount }, () => ({
          x: Math.floor(Math.random() * cols) * width + cx + x,
          y: Math.floor(Math.random() * rows) * height + cy + y,
          delay: Math.random() * 5,
          duration: Math.random() * 3 + 2,
        })),
      );
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [glow, reduced, glowCount, width, height, cx, cy, x, y]);

  return (
    <svg
      ref={containerRef}
      aria-hidden="true"
      className={cn("pointer-events-none absolute inset-0 size-full text-line-2", className)}
      {...props}
    >
      <defs>
        <pattern id={`${id}-dots`} width={width} height={height} patternUnits="userSpaceOnUse" x={x} y={y}>
          <circle cx={cx} cy={cy} r={cr} fill="currentColor" />
        </pattern>
        <radialGradient id={`${id}-glow`}>
          <stop offset="0%" stopColor="#3dff8f" stopOpacity="1" />
          <stop offset="100%" stopColor="#3dff8f" stopOpacity="0" />
        </radialGradient>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${id}-dots)`} />
      {glowDots.map((dot, i) => (
        <motion.circle
          key={i}
          cx={dot.x}
          cy={dot.y}
          r={cr * 2.5}
          fill={`url(#${id}-glow)`}
          initial={{ opacity: 0 }}
          animate={{ opacity: [0, 0.9, 0] }}
          transition={{ duration: dot.duration, repeat: Infinity, repeatType: "loop", delay: dot.delay, ease: "easeInOut" }}
        />
      ))}
    </svg>
  );
}
