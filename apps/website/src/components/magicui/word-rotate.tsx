"use client";

import { AnimatePresence, motion, useReducedMotion, type MotionProps } from "motion/react";
import { useEffect, useState } from "react";

import { cn } from "@/lib/utils";

interface WordRotateProps {
  words: string[];
  /** Milliseconds each word stays. */
  duration?: number;
  motionProps?: MotionProps;
  className?: string;
}

/**
 * Cycles through `words` in place, inline (put it inside a heading). Screen
 * readers get the whole list once; under prefers-reduced-motion the first
 * word stays put.
 */
export function WordRotate({
  words,
  duration = 2600,
  motionProps = {
    initial: { opacity: 0, y: "60%", filter: "blur(6px)" },
    animate: { opacity: 1, y: "0%", filter: "blur(0px)" },
    exit: { opacity: 0, y: "-60%", filter: "blur(6px)" },
    transition: { duration: 0.45, ease: [0.22, 1, 0.36, 1] },
  },
  className,
}: WordRotateProps) {
  const [index, setIndex] = useState(0);
  const reduced = useReducedMotion();

  useEffect(() => {
    if (reduced || words.length < 2) return;
    const interval = setInterval(() => setIndex((i) => (i + 1) % words.length), duration);
    return () => clearInterval(interval);
  }, [words, duration, reduced]);

  return (
    <span className="relative inline-flex overflow-hidden pb-[0.08em] align-bottom">
      <span className="sr-only">{words.join(", ")}</span>
      <AnimatePresence mode="wait" initial={false}>
        <motion.span key={words[index]} aria-hidden className={cn("inline-block", className)} {...motionProps}>
          {words[index]}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}
