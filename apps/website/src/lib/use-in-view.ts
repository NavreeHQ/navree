"use client";

import { useEffect, useState, type RefObject } from "react";

/**
 * True while `ref`'s element is on screen: at least `amount` of it visible
 * (0 to 1), with the viewport grown by `margin` (CSS margin syntax). With
 * `once`, it stays true after the first time. A small IntersectionObserver
 * hook, so components that only need this don't load an animation library.
 */
export function useInView(
  ref: RefObject<Element | null>,
  { amount = 0, once = false, margin }: { amount?: number; once?: boolean; margin?: string } = {},
): boolean {
  const [inView, setInView] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry) return;
        if (entry.isIntersecting) {
          setInView(true);
          if (once) observer.disconnect();
        } else if (!once) {
          setInView(false);
        }
      },
      { threshold: amount, rootMargin: margin },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, amount, once, margin]);

  return inView;
}
