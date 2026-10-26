"use client";

import { motion, useMotionTemplate, useMotionValue, useSpring } from "motion/react";
import { useCallback, useEffect, useRef, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";

import { cn } from "@/lib/utils";
import { useReducedMotion } from "@/lib/use-reduced-motion";

interface MagicCardProps {
  children?: ReactNode;
  className?: string;
  /** "gradient": a spotlight that follows the pointer and lights the border. "orb": a soft blurred glow. */
  mode?: "gradient" | "orb";
  gradientSize?: number;
  /** The spotlight inside the card. */
  gradientColor?: string;
  gradientOpacity?: number;
  /** The border highlight near the pointer. */
  gradientFrom?: string;
  gradientTo?: string;
  glowSize?: number;
  glowBlur?: number;
  glowOpacity?: number;
}

/**
 * A card whose border and surface light up green around the pointer. Give
 * it a radius (`rounded-2xl`) and padding through `className`. Touch and
 * reduced-motion users get the plain card: a 1px line on ink-1.
 */
export function MagicCard({
  children,
  className,
  mode = "gradient",
  gradientSize = 260,
  gradientColor = "rgb(61 255 143 / 0.07)",
  gradientOpacity = 1,
  gradientFrom = "rgb(61 255 143 / 0.55)",
  gradientTo = "rgb(61 255 143 / 0.08)",
  glowSize = 360,
  glowBlur = 70,
  glowOpacity = 0.22,
}: MagicCardProps) {
  const reduced = useReducedMotion();
  const mouseX = useMotionValue(-gradientSize);
  const mouseY = useMotionValue(-gradientSize);

  const orbX = useSpring(mouseX, { stiffness: 250, damping: 30, mass: 0.6 });
  const orbY = useSpring(mouseY, { stiffness: 250, damping: 30, mass: 0.6 });
  const orbVisible = useSpring(0, { stiffness: 300, damping: 35 });

  const gradientSizeRef = useRef(gradientSize);
  useEffect(() => {
    gradientSizeRef.current = gradientSize;
  }, [gradientSize]);

  const reset = useCallback(
    (entering = false) => {
      if (mode === "orb") {
        orbVisible.set(entering ? glowOpacity : 0);
        return;
      }
      mouseX.set(-gradientSizeRef.current);
      mouseY.set(-gradientSizeRef.current);
    },
    [mode, glowOpacity, mouseX, mouseY, orbVisible],
  );

  const handlePointerMove = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      if (e.pointerType !== "mouse") return;
      const rect = e.currentTarget.getBoundingClientRect();
      mouseX.set(e.clientX - rect.left);
      mouseY.set(e.clientY - rect.top);
    },
    [mouseX, mouseY],
  );

  useEffect(() => {
    const handleBlur = () => reset();
    window.addEventListener("blur", handleBlur);
    return () => window.removeEventListener("blur", handleBlur);
  }, [reset]);

  const borderBackground = useMotionTemplate`
    linear-gradient(var(--color-ink-1) 0 0) padding-box,
    radial-gradient(${gradientSize}px circle at ${mouseX}px ${mouseY}px, ${gradientFrom}, ${gradientTo}, var(--color-line) 100%) border-box
  `;
  const spotlight = useMotionTemplate`radial-gradient(${gradientSize}px circle at ${mouseX}px ${mouseY}px, ${gradientColor}, transparent 100%)`;

  if (reduced) {
    return <div className={cn("relative overflow-hidden border border-line bg-ink-1", className)}>{children}</div>;
  }

  return (
    <motion.div
      className={cn("group relative isolate overflow-hidden border border-transparent", className)}
      onPointerMove={handlePointerMove}
      onPointerLeave={() => reset()}
      onPointerEnter={() => reset(true)}
      style={{ background: borderBackground }}
    >
      {mode === "gradient" ? (
        <motion.div
          aria-hidden
          className="pointer-events-none absolute inset-0 z-0 rounded-[inherit] opacity-0 transition-opacity duration-300 group-hover:opacity-100"
          style={{ background: spotlight, opacity: gradientOpacity }}
        />
      ) : (
        <motion.div
          aria-hidden
          className="pointer-events-none absolute top-0 left-0 z-0"
          style={{
            width: glowSize,
            height: glowSize,
            x: orbX,
            y: orbY,
            translateX: "-50%",
            translateY: "-50%",
            borderRadius: 9999,
            filter: `blur(${glowBlur}px)`,
            opacity: orbVisible,
            background: "radial-gradient(circle, rgb(61 255 143 / 0.9), rgb(61 255 143 / 0) 70%)",
            mixBlendMode: "screen",
          }}
        />
      )}
      <div className="relative z-10 h-full">{children}</div>
    </motion.div>
  );
}
