"use client";

import { useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState, type HTMLAttributes } from "react";

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

/**
 * A canvas of small squares that flicker like a field of status lights.
 * Fills its parent; only animates while on screen, and draws one still
 * frame under prefers-reduced-motion. Keep `maxOpacity` low (0.1 to 0.2)
 * behind text.
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
  const [isInView, setIsInView] = useState(false);
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });

  const memoizedColor = useMemo(() => {
    if (typeof window === "undefined") return "rgba(61, 255, 143,";
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const ctx = canvas.getContext("2d");
    if (!ctx) return "rgba(61, 255, 143,";
    ctx.fillStyle = color;
    ctx.fillRect(0, 0, 1, 1);
    const [r, g, b] = Array.from(ctx.getImageData(0, 0, 1, 1).data);
    return `rgba(${r}, ${g}, ${b},`;
  }, [color]);

  const setupCanvas = useCallback(
    (canvas: HTMLCanvasElement, w: number, h: number) => {
      const dpr = window.devicePixelRatio || 1;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
      const cols = Math.ceil(w / (squareSize + gridGap));
      const rows = Math.ceil(h / (squareSize + gridGap));
      const squares = new Float32Array(cols * rows);
      for (let i = 0; i < squares.length; i++) squares[i] = Math.random() * maxOpacity;
      return { cols, rows, squares, dpr };
    },
    [squareSize, gridGap, maxOpacity],
  );

  const updateSquares = useCallback(
    (squares: Float32Array, deltaTime: number) => {
      for (let i = 0; i < squares.length; i++) {
        if (Math.random() < flickerChance * deltaTime) squares[i] = Math.random() * maxOpacity;
      }
    },
    [flickerChance, maxOpacity],
  );

  const drawGrid = useCallback(
    (ctx: CanvasRenderingContext2D, w: number, h: number, cols: number, rows: number, squares: Float32Array, dpr: number) => {
      ctx.clearRect(0, 0, w, h);
      for (let i = 0; i < cols; i++) {
        for (let j = 0; j < rows; j++) {
          ctx.fillStyle = `${memoizedColor}${squares[i * rows + j]})`;
          ctx.fillRect(i * (squareSize + gridGap) * dpr, j * (squareSize + gridGap) * dpr, squareSize * dpr, squareSize * dpr);
        }
      }
    },
    [memoizedColor, squareSize, gridGap],
  );

  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    const ctx = canvas?.getContext("2d") ?? null;
    if (!canvas || !container || !ctx) return;

    let animationFrameId: number | null = null;
    let gridParams: ReturnType<typeof setupCanvas> | null = null;

    const updateCanvasSize = () => {
      const newWidth = width || container.clientWidth;
      const newHeight = height || container.clientHeight;
      setCanvasSize({ width: newWidth, height: newHeight });
      gridParams = setupCanvas(canvas, newWidth, newHeight);
      drawGrid(ctx, canvas.width, canvas.height, gridParams.cols, gridParams.rows, gridParams.squares, gridParams.dpr);
    };
    updateCanvasSize();

    let lastTime = 0;
    const animate = (time: number) => {
      if (!gridParams) return;
      const deltaTime = lastTime ? (time - lastTime) / 1000 : 0;
      lastTime = time;
      updateSquares(gridParams.squares, deltaTime);
      drawGrid(ctx, canvas.width, canvas.height, gridParams.cols, gridParams.rows, gridParams.squares, gridParams.dpr);
      animationFrameId = requestAnimationFrame(animate);
    };

    const resizeObserver = new ResizeObserver(updateCanvasSize);
    resizeObserver.observe(container);
    const intersectionObserver = new IntersectionObserver(([entry]) => setIsInView(entry.isIntersecting), { threshold: 0 });
    intersectionObserver.observe(canvas);

    if (isInView && !reduced) animationFrameId = requestAnimationFrame(animate);

    return () => {
      if (animationFrameId !== null) cancelAnimationFrame(animationFrameId);
      resizeObserver.disconnect();
      intersectionObserver.disconnect();
    };
  }, [setupCanvas, updateSquares, drawGrid, width, height, isInView, reduced]);

  return (
    <div ref={containerRef} aria-hidden className={cn("size-full", className)} {...props}>
      <canvas ref={canvasRef} className="pointer-events-none" style={{ width: canvasSize.width, height: canvasSize.height }} />
    </div>
  );
}
