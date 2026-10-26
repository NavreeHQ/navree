"use client";

import { AnimatePresence, motion, type MotionProps } from "motion/react";
import { Children, memo, useEffect, useMemo, useState, type ComponentPropsWithoutRef, type ReactElement, type ReactNode } from "react";

import { cn } from "@/lib/utils";
import { useReducedMotion } from "@/lib/use-reduced-motion";

export function AnimatedListItem({ children }: { children: ReactNode }) {
  const animations: MotionProps = {
    initial: { scale: 0.96, opacity: 0, y: -8 },
    animate: { scale: 1, opacity: 1, y: 0, originY: 0 },
    exit: { scale: 0.96, opacity: 0 },
    transition: { type: "spring", stiffness: 350, damping: 40 },
  };

  return (
    <motion.div {...animations} layout className="mx-auto w-full">
      {children}
    </motion.div>
  );
}

export interface AnimatedListProps extends ComponentPropsWithoutRef<"div"> {
  children: ReactNode;
  /** Milliseconds between items. */
  delay?: number;
}

/**
 * A feed where items arrive one at a time, newest on top (the Lighthouse
 * issue feed, a job log). Give every child a `key`. Reserve the feed's
 * height (`h-96 overflow-hidden`) so arrivals never shift the page. Under
 * prefers-reduced-motion the full list shows at once.
 */
export const AnimatedList = memo(function AnimatedList({ children, className, delay = 1400, ...props }: AnimatedListProps) {
  const reduced = useReducedMotion();
  const [index, setIndex] = useState(0);
  const childrenArray = useMemo(() => Children.toArray(children), [children]);

  useEffect(() => {
    if (reduced || index >= childrenArray.length - 1) return;
    const timeout = setTimeout(() => setIndex((i) => (i + 1) % childrenArray.length), delay);
    return () => clearTimeout(timeout);
  }, [index, delay, childrenArray.length, reduced]);

  const itemsToShow = useMemo(
    () => (reduced ? childrenArray.slice().reverse() : childrenArray.slice(0, index + 1).reverse()),
    [index, childrenArray, reduced],
  );

  return (
    <div className={cn("flex flex-col items-center gap-3", className)} {...props}>
      <AnimatePresence>
        {itemsToShow.map((item) => (
          <AnimatedListItem key={(item as ReactElement).key}>{item}</AnimatedListItem>
        ))}
      </AnimatePresence>
    </div>
  );
});
