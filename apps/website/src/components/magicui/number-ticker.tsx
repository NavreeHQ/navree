"use client";

import { useEffect, useLayoutEffect, useRef, type ComponentPropsWithoutRef } from "react";

import { useInView } from "@/lib/use-in-view";
import { springRemaining } from "@/lib/spring";
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

/** The count's spring (motion's `useSpring` with these settings): heavily damped, so it eases in without overshooting. */
const remaining = springRemaining({ stiffness: 100, damping: 60 });

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

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
  const from = direction === "down" ? value : startValue;
  const to = direction === "down" ? startValue : value;
  const isInView = useInView(ref, { once: true, margin: "0px" });

  // Before the first paint, reset to the start value so the count can play.
  useLayoutEffect(() => {
    if (reducedMotion() || !ref.current) return;
    ref.current.textContent = format(from, decimalPlaces);
  }, [from, decimalPlaces]);

  useEffect(() => {
    if (reducedMotion() || !isInView) return;
    let frame = 0;
    // Rest thresholds as motion sets them: finer for small ranges.
    const restDelta = Math.abs(to - from) < 5 ? 0.005 : 0.5;
    const timer = setTimeout(() => {
      const start = performance.now();
      const tick = (now: number) => {
        const left = (to - from) * remaining(now - start);
        if (ref.current) ref.current.textContent = format(Math.abs(left) <= restDelta ? to : to - left, decimalPlaces);
        if (Math.abs(left) > restDelta) frame = requestAnimationFrame(tick);
      };
      frame = requestAnimationFrame(tick);
    }, delay * 1000);
    return () => {
      clearTimeout(timer);
      cancelAnimationFrame(frame);
    };
  }, [isInView, delay, from, to, decimalPlaces]);

  return (
    <span ref={ref} className={cn("inline-block tabular-nums", className)} {...props}>
      {format(to, decimalPlaces)}
    </span>
  );
}
