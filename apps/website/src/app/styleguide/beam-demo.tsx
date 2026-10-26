"use client";

import { useRef } from "react";

import { AnimatedBeam } from "@/components/magicui/animated-beam";
import { cn } from "@/lib/utils";

function Node({ children, accent, ref }: { children: string; accent?: boolean; ref: React.Ref<HTMLDivElement> }) {
  return (
    <div
      ref={ref}
      className={cn(
        "z-10 rounded-full border bg-ink-1 px-3.5 py-1.5 font-mono text-xs",
        accent ? "border-accent text-accent shadow-glow-sm" : "border-line-2 text-fg-1",
      )}
    >
      {children}
    </div>
  );
}

/** A request flowing client → router → module, for the styleguide. */
export function BeamDemo() {
  const container = useRef<HTMLDivElement>(null);
  const a = useRef<HTMLDivElement>(null);
  const b = useRef<HTMLDivElement>(null);
  const c = useRef<HTMLDivElement>(null);
  return (
    <div ref={container} className="relative flex h-40 items-center justify-between overflow-hidden rounded-2xl border border-line bg-ink-0 px-6 sm:px-12">
      <Node ref={a}>client</Node>
      <Node ref={b} accent>
        router
      </Node>
      <Node ref={c}>DealsModule</Node>
      <AnimatedBeam containerRef={container} fromRef={a} toRef={b} />
      <AnimatedBeam containerRef={container} fromRef={b} toRef={c} delay={0.6} />
    </div>
  );
}
