"use client";

import { useInView, useMotionValue, useReducedMotion, useSpring } from "motion/react";
import { useEffect, useLayoutEffect, useRef, type ComponentPropsWithoutRef } from "react";

import { cn } from "@/lib/utils";

interface NumberTickerProps extends ComponentPropsWithoutRef<"span"> {
  value: number;
  startValue?: number;
  direction?: "up" | "down";
  /** Seconds to wait after the number scrolls into view. */
  delay?: number;
  decimalPlaces?: number;
}

const format = (n: number, decimalPlaces: number) =>
  Intl.NumberFormat("en-US", { minimumFractionDigits: decimalPlaces, maximumFractionDigits: decimalPlaces }).format(
    Number(n.toFixed(decimalPlaces)),
  );

/**
 * A number that counts up when it scrolls into view. The static HTML holds
 * the final value (crawlers, no-JS, reduced motion all see the real number);
 * tabular figures keep the width steady while it counts.
 */
export function NumberTicker({
  value,
  startValue = 0,
  direction = "up",
  delay = 0,
  className,
  decimalPlaces = 0,
  ...props
}: NumberTickerProps) {
  const ref = useRef<HTMLSpanElement>(null);
  const reduced = useReducedMotion();
  const from = direction === "down" ? value : startValue;
  const to = direction === "down" ? startValue : value;
  const motionValue = useMotionValue(from);
  const springValue = useSpring(motionValue, { damping: 60, stiffness: 100 });
  const isInView = useInView(ref, { once: true, margin: "0px" });

  // Before the first paint, reset to the start value so the count can play.
  useLayoutEffect(() => {
    if (reduced || !ref.current) return;
    ref.current.textContent = format(from, decimalPlaces);
  }, [reduced, from, decimalPlaces]);

  useEffect(() => {
    if (reduced || !isInView) return;
    const timer = setTimeout(() => motionValue.set(to), delay * 1000);
    return () => clearTimeout(timer);
  }, [motionValue, isInView, delay, to, reduced]);

  useEffect(
    () =>
      springValue.on("change", (latest) => {
        if (ref.current) ref.current.textContent = format(latest, decimalPlaces);
      }),
    [springValue, decimalPlaces],
  );

  return (
    <span ref={ref} className={cn("inline-block tabular-nums", className)} {...props}>
      {format(to, decimalPlaces)}
    </span>
  );
}
