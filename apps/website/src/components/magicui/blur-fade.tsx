"use client";

import { motion, useInView, type MotionProps, type UseInViewOptions, type Variants } from "motion/react";
import { useRef, type ReactNode } from "react";

import { useReducedMotion } from "@/lib/use-reduced-motion";
import { cn } from "@/lib/utils";

type MarginType = UseInViewOptions["margin"];

interface BlurFadeProps extends MotionProps {
  children: ReactNode;
  className?: string;
  variant?: { hidden: { y: number }; visible: { y: number } };
  /** Seconds. */
  duration?: number;
  /** Seconds; stagger siblings with 0.06 to 0.1 steps. */
  delay?: number;
  offset?: number;
  direction?: "up" | "down" | "left" | "right";
  /** Wait until the element scrolls into view (the usual case below the fold). */
  inView?: boolean;
  inViewMargin?: MarginType;
  blur?: string;
}

const getFilter = (v: Variants[string]) => (typeof v === "function" ? undefined : v.filter);

/**
 * The section entrance: a short fade up out of a light blur. Transform and
 * opacity only, so nothing shifts. Under prefers-reduced-motion it renders
 * in place. Never wrap the hero headline or anything above the fold that
 * people read first.
 */
export function BlurFade({
  children,
  className,
  variant,
  duration = 0.6,
  delay = 0,
  offset = 12,
  direction = "up",
  inView = true,
  inViewMargin = "-60px",
  blur = "6px",
  ...props
}: BlurFadeProps) {
  const ref = useRef(null);
  const reduced = useReducedMotion();
  const inViewResult = useInView(ref, { once: true, margin: inViewMargin });
  const isInView = !inView || inViewResult;
  const axis = direction === "left" || direction === "right" ? "x" : "y";
  const defaultVariants: Variants = {
    hidden: { [axis]: direction === "right" || direction === "down" ? -offset : offset, opacity: 0, filter: `blur(${blur})` },
    visible: { [axis]: 0, opacity: 1, filter: "blur(0px)" },
  };
  const combinedVariants = variant ?? defaultVariants;

  const hiddenFilter = getFilter(combinedVariants.hidden);
  const visibleFilter = getFilter(combinedVariants.visible);
  const shouldTransitionFilter = hiddenFilter != null && visibleFilter != null && hiddenFilter !== visibleFilter;

  if (reduced) {
    return <div className={className}>{children}</div>;
  }

  return (
    <motion.div
      ref={ref}
      initial="hidden"
      animate={isInView ? "visible" : "hidden"}
      variants={combinedVariants}
      transition={{
        delay: 0.04 + delay,
        duration,
        ease: [0.22, 1, 0.36, 1],
        ...(shouldTransitionFilter ? { filter: { duration } } : {}),
      }}
      // Before the page's JavaScript runs, reduced-motion visitors still see the content.
      className={cn(className, "motion-reduce:opacity-100! motion-reduce:transform-none! motion-reduce:filter-none!")}
      {...props}
    >
      {children}
    </motion.div>
  );
}
