"use client";

import { useLayoutEffect, useRef, type RefObject } from "react";

import { springKeyframes } from "@/lib/spring";

/**
 * The feed's motion, as the Magic UI animated list draws it with the
 * `motion` library (a spring of stiffness 350 and damping 40), without
 * loading the library: a new item fades and grows in from just above its
 * place, and the items already there glide down to their new places. Each
 * child of the container needs a `data-key`. The first render doesn't
 * animate, and nothing does under prefers-reduced-motion.
 */
const spring = springKeyframes({ stiffness: 350, damping: 40 });

export function useFlipList(container: RefObject<HTMLElement | null>, deps: unknown[]) {
  const tops = useRef<Map<string, number> | null>(null);

  useLayoutEffect(() => {
    const el = container.current;
    if (!el) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const previous = tops.current;
    const next = new Map<string, number>();
    for (const child of Array.from(el.children) as HTMLElement[]) {
      const key = child.dataset.key;
      if (!key) continue;
      next.set(key, child.offsetTop);
      if (!previous || reduced || typeof child.animate !== "function") continue;
      const before = previous.get(key);
      if (before === undefined) {
        child.animate(
          spring.frames.map(({ offset, progress: p }) => ({
            offset,
            opacity: p,
            transform: `translateY(${-8 * (1 - p)}px) scale(${0.96 + 0.04 * p})`,
            transformOrigin: "50% 0",
          })),
          { duration: spring.duration, easing: "linear" },
        );
      } else if (before !== child.offsetTop) {
        const shift = before - child.offsetTop;
        child.animate(
          spring.frames.map(({ offset, progress: p }) => ({ offset, transform: `translateY(${shift * (1 - p)}px)` })),
          { duration: spring.duration, easing: "linear" },
        );
      }
    }
    tops.current = next;
    // The caller lists what changes the items.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
