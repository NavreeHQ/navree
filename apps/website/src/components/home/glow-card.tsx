"use client";

import { useRef, type PointerEvent, type ReactNode } from "react";

import { cn } from "@/lib/utils";

/**
 * A card whose border and surface light up green around a mouse pointer.
 * The pointer position is two CSS variables set on move, so the server and
 * the client render the same tree; touch screens and reduced motion get
 * the plain card (home.css hides the light).
 */
export function GlowCard({ children, className }: { children: ReactNode; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const el = ref.current;
    if (!el || event.pointerType !== "mouse") return;
    const rect = el.getBoundingClientRect();
    el.style.setProperty("--mx", `${event.clientX - rect.left}px`);
    el.style.setProperty("--my", `${event.clientY - rect.top}px`);
  }

  return (
    <div
      ref={ref}
      onPointerMove={onPointerMove}
      className={cn("glow-card group relative isolate overflow-hidden border border-line bg-ink-1 transition-colors duration-300 hover:border-line-2", className)}
    >
      <div aria-hidden className="glow-card-spot" />
      <div aria-hidden className="glow-card-ring" />
      <div className="relative z-10 h-full">{children}</div>
    </div>
  );
}
