"use client";

import { useEffect, useRef, type HTMLAttributes } from "react";

import { useReducedMotion } from "@/lib/use-reduced-motion";
import { cn } from "@/lib/utils";

interface FlickeringGridProps extends HTMLAttributes<HTMLDivElement> {
  squareSize?: number;
  gridGap?: number;
  /** Chance per second that a square changes. */
  flickerChance?: number;
  /** Any CSS color; Navree green by default. */
  color?: string;
  width?: number;
  height?: number;
  className?: string;
  maxOpacity?: number;
}

/** `#rgb` or `#rrggbb` as "r, g, b"; anything else is resolved by the canvas itself. */
function hexToRgb(color: string): string | null {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color.trim())?.[1];
  if (!hex) return null;
  const full = hex.length === 3 ? [...hex].map((c) => c + c).join("") : hex;
  return [0, 2, 4].map((i) => Number.parseInt(full.slice(i, i + 2), 16)).join(", ");
}

/**
 * A canvas of small squares that flicker like a field of status lights.
 * Fills its parent; draws nothing until it comes near the screen, only
 * animates while on screen, and draws one still frame under
 * prefers-reduced-motion. Keep `maxOpacity` low (0.1 to 0.2) behind text.
 */
export function FlickeringGrid({
  squareSize = 3,
  gridGap = 5,
  flickerChance = 0.25,
  color = "#3dff8f",
  width,
  height,
  className,
  maxOpacity = 0.15,
  ...props
}: FlickeringGridProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();

  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;

    let ctx: CanvasRenderingContext2D | null = null;
    let fill = "";
    let grid: { cols: number; rows: number; squares: Float32Array; dpr: number } | null = null;
    let frame: number | null = null;
    let lastTime = 0;
    let resizeObserver: ResizeObserver | null = null;

    const setup = () => {
      if (!ctx) return;
      const w = width || container.clientWidth;
      const h = height || container.clientHeight;
      const dpr = window.devicePixelRatio || 1;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
      const cols = Math.ceil(w / (squareSize + gridGap));
      const rows = Math.ceil(h / (squareSize + gridGap));
      const squares = new Float32Array(cols * rows);
      for (let i = 0; i < squares.length; i++) squares[i] = Math.random() * maxOpacity;
      grid = { cols, rows, squares, dpr };
      draw();
    };

    // One fill color; each square's opacity is the global alpha (the same pixels as an rgba fill per square).
    const draw = () => {
      if (!ctx || !grid) return;
      const { cols, rows, squares, dpr } = grid;
      const step = (squareSize + gridGap) * dpr;
      const size = squareSize * dpr;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.fillStyle = fill;
      for (let i = 0; i < cols; i++) {
        for (let j = 0; j < rows; j++) {
          ctx.globalAlpha = squares[i * rows + j];
          ctx.fillRect(i * step, j * step, size, size);
        }
      }
      ctx.globalAlpha = 1;
    };

    const animate = (time: number) => {
      if (!grid) return;
      const delta = lastTime ? (time - lastTime) / 1000 : 0;
      lastTime = time;
      const { squares } = grid;
      for (let i = 0; i < squares.length; i++) {
        if (Math.random() < flickerChance * delta) squares[i] = Math.random() * maxOpacity;
      }
      draw();
      frame = requestAnimationFrame(animate);
    };

    const start = () => {
      if (!ctx) {
        ctx = canvas.getContext("2d");
        if (!ctx) return;
        const rgb = hexToRgb(color);
        if (rgb) {
          fill = `rgb(${rgb})`;
        } else {
          ctx.fillStyle = color;
          fill = ctx.fillStyle;
        }
        setup();
        resizeObserver = new ResizeObserver(setup);
        resizeObserver.observe(container);
      }
      if (!reduced && frame === null) {
        lastTime = 0;
        frame = requestAnimationFrame(animate);
      }
    };

    const stop = () => {
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
    };

    // Set the canvas up just before it scrolls into view; animate only while it is on screen.
    const observer = new IntersectionObserver(([entry]) => (entry?.isIntersecting ? start() : stop()), { rootMargin: "200px 0px" });
    observer.observe(container);

    return () => {
      stop();
      observer.disconnect();
      resizeObserver?.disconnect();
    };
  }, [squareSize, gridGap, flickerChance, color, width, height, maxOpacity, reduced]);

  return (
    <div ref={containerRef} aria-hidden className={cn("size-full", className)} {...props}>
      <canvas ref={canvasRef} className="pointer-events-none" style={{ width: 0, height: 0 }} />
    </div>
  );
}
