"use client";

import { motion, useReducedMotion } from "motion/react";

import { cn } from "@/lib/utils";

const EASE = [0.45, 0, 0.25, 1] as const;

/**
 * A connector with an arrowhead and a short streak of green light running
 * along it in the direction of flow: a request going out, a query going to
 * the database. Vertical on phones, horizontal from `lg`. The streak moves
 * by transform only; under prefers-reduced-motion it is hidden and still.
 * The markup is the same on the server and the client either way.
 */
export function FlowLine({ delay = 0, className }: { delay?: number; className?: string }) {
  const reduced = useReducedMotion();
  const transition = { duration: 2.4, delay, ease: EASE, repeat: Infinity, repeatDelay: 0.6 };
  return (
    <span aria-hidden className={cn("relative block", className)}>
      {/* The line. */}
      <span className="absolute top-0 bottom-2 left-1/2 w-px -translate-x-1/2 bg-line-2 lg:top-1/2 lg:right-2 lg:bottom-auto lg:left-0 lg:h-px lg:w-auto lg:translate-x-0 lg:-translate-y-1/2" />
      {/* The arrowhead: down on phones, right from lg. */}
      <svg
        viewBox="0 0 10 10"
        className="absolute bottom-0 left-1/2 size-2.5 -translate-x-1/2 rotate-90 text-fg-3 lg:top-1/2 lg:right-0 lg:bottom-auto lg:left-auto lg:translate-x-0 lg:-translate-y-1/2 lg:rotate-0"
      >
        <path d="M1 1 L8 5 L1 9" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      {/* The streak. */}
      <span className="absolute top-0 bottom-2 left-1/2 w-[3px] -translate-x-1/2 overflow-hidden rounded-full motion-reduce:hidden lg:top-1/2 lg:right-2 lg:bottom-auto lg:left-0 lg:h-[3px] lg:w-auto lg:translate-x-0 lg:-translate-y-1/2">
        <motion.span
          className="absolute inset-x-0 top-0 h-2/5 bg-linear-to-b from-transparent via-accent to-transparent lg:hidden"
          initial={{ y: "-100%" }}
          animate={reduced ? undefined : { y: "250%" }}
          transition={transition}
        />
        <motion.span
          className="absolute inset-y-0 left-0 hidden w-2/5 bg-linear-to-r from-transparent via-accent to-transparent lg:block"
          initial={{ x: "-100%" }}
          animate={reduced ? undefined : { x: "250%" }}
          transition={transition}
        />
      </span>
    </span>
  );
}
