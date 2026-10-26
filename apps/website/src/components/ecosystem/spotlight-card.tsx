"use client";

import { useRef, type CSSProperties, type PointerEvent, type ReactNode } from "react";

import { cn } from "@/lib/utils";

const OFF = "-999px";

/**
 * A card whose 1px border and surface light up green around the pointer,
 * the Magic Card look. The pointer position lives in CSS variables set on
 * the element, so moving the mouse never re-renders React, and the server
 * and the client always render the same markup. Touch and reduced-motion
 * users get the plain card: a 1px line on ink-1.
 */
export function SpotlightCard({ children, className, size = 260 }: { children: ReactNode; className?: string; size?: number }) {
  const ref = useRef<HTMLDivElement>(null);

  function move(e: PointerEvent<HTMLDivElement>) {
    if (e.pointerType !== "mouse" || !ref.current) return;
    const rect = ref.current.getBoundingClientRect();
    ref.current.style.setProperty("--spot-x", `${e.clientX - rect.left}px`);
    ref.current.style.setProperty("--spot-y", `${e.clientY - rect.top}px`);
  }

  function leave() {
    ref.current?.style.setProperty("--spot-x", OFF);
    ref.current?.style.setProperty("--spot-y", OFF);
  }

  return (
    <div
      ref={ref}
      onPointerMove={move}
      onPointerLeave={leave}
      style={{ "--spot-x": OFF, "--spot-y": OFF, "--spot-size": `${size}px` } as CSSProperties}
      className={cn(
        "group/spot relative isolate overflow-hidden border border-transparent shadow-bezel",
        "[background:linear-gradient(var(--color-ink-1)_0_0)_padding-box,radial-gradient(var(--spot-size)_circle_at_var(--spot-x)_var(--spot-y),rgb(61_255_143/0.55),rgb(61_255_143/0.08),var(--color-line)_100%)_border-box]",
        "motion-reduce:border-line motion-reduce:[background:var(--color-ink-1)]",
        className,
      )}
    >
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 rounded-[inherit] opacity-0 transition-opacity duration-300 group-hover/spot:opacity-100 motion-reduce:hidden [background:radial-gradient(var(--spot-size)_circle_at_var(--spot-x)_var(--spot-y),rgb(61_255_143/0.07),transparent_100%)]"
      />
      {children}
    </div>
  );
}
