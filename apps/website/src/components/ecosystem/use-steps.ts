"use client";

import { useEffect, useRef, useState } from "react";

import { useInView } from "@/lib/use-in-view";
import { useReducedMotion } from "@/lib/use-reduced-motion";

type StepOptions = {
  /** Milliseconds before moving on from a step: one value, or one per step. */
  interval: number | ((step: number) => number);
  /** Start again from 0 after the last step, holding it for `hold` ms. */
  loop?: boolean;
  hold?: number;
  /** The step rendered on the server and kept under reduced motion. Defaults to the last. */
  still?: number;
};

function isOnScreen(el: Element): boolean {
  const r = el.getBoundingClientRect();
  return r.top < window.innerHeight && r.bottom > 0;
}

/**
 * Drives a product mock through `count` steps while it is on screen.
 *
 * The server renders the `still` step, so the mock reads complete without
 * JavaScript. After hydration, a mock that is off screen rewinds to step 0
 * (where nobody sees it happen) and plays when it scrolls into view; one
 * already on screen keeps its still step and, when looping, starts over
 * after `hold`. It pauses off screen, and never moves under reduced motion.
 */
export function useSteps<T extends Element = HTMLDivElement>(count: number, options: StepOptions) {
  const { interval, loop = true, hold = 2600, still = count - 1 } = options;
  const ref = useRef<T>(null);
  const reduced = useReducedMotion();
  const inView = useInView(ref, { amount: 0.3 });
  const [step, setStep] = useState(still);

  useEffect(() => {
    if (reduced) return;
    const frame = requestAnimationFrame(() => {
      if (ref.current && !isOnScreen(ref.current)) setStep(0);
    });
    return () => cancelAnimationFrame(frame);
  }, [reduced]);

  useEffect(() => {
    if (reduced || !inView) return;
    const last = count - 1;
    if (step >= last && !loop) return;
    const wait = step >= last ? hold : typeof interval === "function" ? interval(step) : interval;
    const timer = setTimeout(() => setStep((s) => (s >= last ? 0 : s + 1)), wait);
    return () => clearTimeout(timer);
  }, [step, inView, reduced, count, loop, hold, interval]);

  return { ref, step: reduced ? still : step, reduced: !!reduced };
}

/**
 * True once an element that started off screen has scrolled into view: the
 * moment to play a one-off entrance. Elements already on screen when the
 * page loads, and everyone under reduced motion, never play it.
 */
export function usePlayOnView<T extends Element = HTMLDivElement>() {
  const ref = useRef<T>(null);
  const reduced = useReducedMotion();
  const inView = useInView(ref, { amount: 0.25, once: true });
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    if (reduced) return;
    const frame = requestAnimationFrame(() => {
      if (ref.current && !isOnScreen(ref.current)) setArmed(true);
    });
    return () => cancelAnimationFrame(frame);
  }, [reduced]);

  return { ref, armed: armed && !reduced, play: armed && inView && !reduced };
}
