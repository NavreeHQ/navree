"use client";

import { useEffect, useRef } from "react";

import { lightAt } from "./picture-light";
import { HEAD, LEVELS, NARROW_BELOW, VIEW, cells, firstDrops, grids, levels as levelsOf, newDrop, trailLevel, type Drop, type Grid } from "./picture-rain";

/**
 * The digital rain of the about picture: a canvas over the still SVG that
 * makes the digits in the figures fall and change. It starts from the same
 * frame the SVG draws (the same seeds), then redraws only the cells that
 * change, twelve times a second.
 *
 * Kept light on purpose:
 * - nothing runs under `prefers-reduced-motion`, off screen or in a hidden
 *   tab; the still picture stays;
 * - one canvas at most 1.6 million device pixels, a 4-glyph atlas, and no
 *   layout reads or style changes per frame (sizes are read when the
 *   picture is resized);
 * - each frame clears and draws only the cells whose digit or brightness
 *   changed: a few hundred small images.
 */

const FPS = 12;
/** Digits that change at random each frame, besides those a falling drop passes. */
const FLIPS = 3;
/** The canvas's largest backing store, in device pixels. */
const MAX_PIXELS = 1_600_000;
/** Ticks a column waits, at most, before a new drop starts in it. */
const MAX_WAIT = 36;
const GREEN = "#3dff8f";
const MINT = "#d9ffe8";

type Scene = { start(): void; stop(): void; dispose(): void };

/** A figure layer read back from the SVG: its paths and the gap it cuts into what is behind it. */
type Layer = { halo: number; paths: Path2D[] };
/** The figures read back from the SVG: their layers, and the shapes (the backrest) that dim what is under them. */
type Figures = { layers: Layer[]; veils: { path: Path2D; amount: number }[] };

/**
 * Paints the figures in white, as the SVG's mask does: each layer cuts its
 * gap into the ones behind, then fills itself; a veil then takes `amount`
 * of what is under it away.
 */
function paintFigures(ctx: CanvasRenderingContext2D, { layers, veils }: Figures) {
  ctx.fillStyle = "#fff";
  ctx.lineJoin = "round";
  for (const layer of layers) {
    if (layer.halo) {
      ctx.globalCompositeOperation = "destination-out";
      ctx.lineWidth = layer.halo * 2;
      for (const path of layer.paths) {
        ctx.fill(path);
        ctx.stroke(path);
      }
    }
    ctx.globalCompositeOperation = "source-over";
    for (const path of layer.paths) ctx.fill(path);
  }
  ctx.globalCompositeOperation = "destination-out";
  for (const veil of veils) {
    ctx.globalAlpha = veil.amount;
    ctx.fill(veil.path);
  }
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";
}

/** Which cells the figures cover, sampled 4 x 4 per cell: 0 none, 1 the edge, 2 wholly inside. */
function coverage(g: Grid, figures: Figures): Uint8Array {
  const S = 4;
  const probe = document.createElement("canvas");
  probe.width = g.cols * S;
  probe.height = g.rows * S;
  const ctx = probe.getContext("2d", { willReadFrequently: true });
  const out = new Uint8Array(g.cols * g.rows);
  if (!ctx) return out;
  ctx.setTransform(S / g.cw, 0, 0, S / g.ch, (-g.x * S) / g.cw, (-g.y * S) / g.ch);
  paintFigures(ctx, figures);
  const alpha = ctx.getImageData(0, 0, probe.width, probe.height).data;
  for (let row = 0; row < g.rows; row++) {
    for (let col = 0; col < g.cols; col++) {
      let min = 255;
      let max = 0;
      for (let sy = 0; sy < S; sy++) {
        for (let sx = 0; sx < S; sx++) {
          const a = alpha[((row * S + sy) * probe.width + col * S + sx) * 4 + 3];
          if (a < min) min = a;
          if (a > max) max = a;
        }
      }
      out[row * g.cols + col] = max < 8 ? 0 : min > 247 ? 2 : 1;
    }
  }
  probe.width = probe.height = 0;
  return out;
}

/** Sets the rain up on `canvas` for a picture `width` by `height` CSS pixels; `null` if the canvas can't draw. */
async function build(canvas: HTMLCanvasElement, svg: SVGSVGElement, width: number, height: number): Promise<Scene | null> {
  const g = width < NARROW_BELOW ? grids[1] : grids[0];
  const dpr = Math.min(window.devicePixelRatio || 1, 2, Math.sqrt(MAX_PIXELS / (width * height)));
  const k = (width / VIEW.width) * dpr;
  const family = getComputedStyle(svg).fontFamily;
  const fontPx = g.font * k;
  try {
    await document.fonts.load(`${fontPx}px ${family}`, "01");
  } catch {
    // The fallback face draws the same digits.
  }

  const figures: Figures = {
    layers: [...svg.querySelectorAll<SVGGElement>("g[data-halo]")].map((el) => ({
      halo: Number(el.dataset.halo) || 0,
      paths: [...el.querySelectorAll("path")].map((p) => new Path2D(p.getAttribute("d") ?? "")),
    })),
    veils: [...svg.querySelectorAll<SVGPathElement>("path[data-veil]")].map((el) => ({
      path: new Path2D(el.getAttribute("d") ?? ""),
      amount: Number(el.dataset.veil) || 0,
    })),
  };
  // Cells under a veil count as edges: they keep their first frame, dimmed.
  const covered = coverage(g, figures);

  // The atlas: "0" and "1", each in green and in mint (a drop's head).
  const cw = Math.ceil(g.cw * k);
  const ch = Math.ceil(g.ch * k);
  const atlas = document.createElement("canvas");
  atlas.width = cw * 4;
  atlas.height = ch;
  const actx = atlas.getContext("2d");
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  const ctx = canvas.getContext("2d");
  if (!actx || !ctx) return null;
  actx.font = `${fontPx}px ${family}`;
  actx.textAlign = "center";
  ["0", "1"].forEach((digit, d) =>
    [GREEN, MINT].forEach((color, c) => {
      actx.fillStyle = color;
      actx.fillText(digit, (d * 2 + c) * cw + (g.cw * k) / 2, g.ch * 0.78 * k);
    }),
  );

  const n = g.cols * g.rows;
  const left = (col: number) => Math.round((g.x + col * g.cw - VIEW.x) * k);
  const top = (row: number) => Math.round((g.y + row * g.ch - VIEW.y) * k);
  const light = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const col = i % g.cols;
    const row = (i - col) / g.cols;
    light[i] = lightAt(g.x + (col + 0.5) * g.cw, g.y + (row + 0.5) * g.ch);
  }

  const { chars, base } = cells(g);
  const digits = Uint8Array.from(chars, (c) => (c === "1" ? 1 : 0));
  const first = firstDrops(g);
  const byCol: (Drop | null)[] = Array.from({ length: g.cols }, () => null);
  for (const drop of first) byCol[drop.col] = drop;
  const level = levelsOf(g, base, first);
  const wait = Uint8Array.from({ length: g.cols }, () => Math.floor(Math.random() * MAX_WAIT));

  const draw = (i: number, clear: boolean) => {
    const col = i % g.cols;
    const row = (i - col) / g.cols;
    const x = left(col);
    const y = top(row);
    if (clear) ctx.clearRect(x, y, left(col + 1) - x, top(row + 1) - y);
    const head = level[i] === HEAD;
    ctx.globalAlpha = head ? 1 : LEVELS[level[i]] * light[i];
    ctx.drawImage(atlas, (digits[i] * 2 + (head ? 1 : 0)) * cw, 0, cw, ch, x, y, cw, ch);
  };

  // The first frame, then the figures' outline cut out of it (edges and the gaps between them).
  for (let i = 0; i < n; i++) if (covered[i]) draw(i, false);
  const mask = document.createElement("canvas");
  mask.width = canvas.width;
  mask.height = canvas.height;
  const mctx = mask.getContext("2d");
  if (mctx) {
    mctx.setTransform(k, 0, 0, k, -VIEW.x * k, -VIEW.y * k);
    paintFigures(mctx, figures);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = "destination-in";
    ctx.drawImage(mask, 0, 0);
    ctx.globalCompositeOperation = "source-over";
  }
  mask.width = mask.height = 0;

  // Only cells wholly inside a figure change; the edges keep their first frame.
  const inside: number[] = [];
  for (let i = 0; i < n; i++) if (covered[i] === 2) inside.push(i);
  const columns = [...new Set(inside.map((i) => i % g.cols))];
  const dirty = new Uint8Array(n);
  const queue: number[] = [];
  const touch = (i: number) => {
    if (covered[i] === 2 && !dirty[i]) {
      dirty[i] = 1;
      queue.push(i);
    }
  };

  const tick = () => {
    for (const col of columns) {
      let drop = byCol[col];
      if (!drop) {
        if (wait[col] > 0) wait[col]--;
        else byCol[col] = drop = newDrop(g, col, Math.random, false);
        if (!drop) continue;
      }
      const before = Math.floor(drop.head);
      drop.head += drop.speed;
      const after = Math.floor(drop.head);
      if (after === before) continue;
      if (after >= 0 && after < g.rows) digits[after * g.cols + col] = Math.random() < 0.5 ? 1 : 0;
      for (let row = Math.max(0, after - drop.length); row <= Math.min(g.rows - 1, after); row++) touch(row * g.cols + col);
      if (after - drop.length >= g.rows) {
        byCol[col] = null;
        wait[col] = Math.floor(Math.random() * MAX_WAIT);
      }
    }
    for (let f = 0; f < FLIPS && inside.length; f++) {
      const i = inside[Math.floor(Math.random() * inside.length)];
      digits[i] ^= 1;
      touch(i);
    }
    for (const i of queue) {
      dirty[i] = 0;
      const col = i % g.cols;
      const row = (i - col) / g.cols;
      const drop = byCol[col];
      level[i] = Math.max(base[i], drop ? trailLevel(Math.floor(drop.head) - row, drop.length) : -1);
      draw(i, true);
    }
    queue.length = 0;
  };

  let running = false;
  let timer = 0;
  let raf = 0;
  const loop = () => {
    timer = window.setTimeout(() => {
      raf = requestAnimationFrame(() => {
        tick();
        if (running) loop();
      });
    }, 1000 / FPS - 8);
  };
  const stop = () => {
    running = false;
    window.clearTimeout(timer);
    cancelAnimationFrame(raf);
  };
  return {
    start() {
      if (running) return;
      running = true;
      loop();
    },
    stop,
    dispose() {
      stop();
      canvas.width = canvas.height = 0;
      atlas.width = atlas.height = 0;
    },
  };
}

/**
 * The canvas of the rain. It sits over the still picture in the same frame
 * (its parent), draws nothing until the picture comes near the screen, and
 * marks the frame `data-live` while it draws, which hides the still digits.
 */
export function FamilyRain({ className }: { className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    const frame = canvas?.parentElement;
    const svg = frame?.querySelector("svg");
    if (!canvas || !frame || !svg) return;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let scene: Scene | null = null;
    // Each build (and each drop) takes a new generation; a build that finishes after a newer one started is thrown away.
    let generation = 0;
    let pending = -1;
    let onScreen = false;
    let size = { width: 0, height: 0 };
    let resizeTimer = 0;

    const wanted = () => onScreen && !motion.matches && document.visibilityState === "visible" && size.width > 0;
    const drop = () => {
      generation++;
      scene?.dispose();
      scene = null;
      delete frame.dataset.live;
    };
    const sync = (): void => {
      if (motion.matches) {
        drop();
      } else if (!wanted()) {
        scene?.stop();
      } else if (scene) {
        scene.start();
      } else if (pending !== generation) {
        const mine = ++generation;
        pending = mine;
        const idle = window.requestIdleCallback ?? ((fn: () => void) => window.setTimeout(fn, 1));
        idle(() => {
          if (mine !== generation || !wanted()) {
            if (pending === mine) pending = -1;
            return;
          }
          void build(canvas, svg, size.width, size.height).then((built) => {
            if (pending === mine) pending = -1;
            if (mine !== generation) {
              built?.dispose();
            } else if (built) {
              scene = built;
              frame.dataset.live = "";
              sync();
            }
          });
        });
      }
    };

    const io = new IntersectionObserver(
      ([entry]) => {
        onScreen = Boolean(entry?.isIntersecting);
        sync();
      },
      { rootMargin: "120px 0px" },
    );
    const ro = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const next = { width: Math.round(entry.contentRect.width), height: Math.round(entry.contentRect.height) };
      if (next.width === size.width && next.height === size.height) return;
      const first = size.width === 0;
      size = next;
      window.clearTimeout(resizeTimer);
      if (first) sync();
      else
        resizeTimer = window.setTimeout(() => {
          drop();
          sync();
        }, 200);
    });
    io.observe(frame);
    ro.observe(frame);
    document.addEventListener("visibilitychange", sync);
    motion.addEventListener("change", sync);
    return () => {
      io.disconnect();
      ro.disconnect();
      document.removeEventListener("visibilitychange", sync);
      motion.removeEventListener("change", sync);
      window.clearTimeout(resizeTimer);
      drop();
    };
  }, []);

  return <canvas ref={ref} width={0} height={0} aria-hidden className={className} />;
}
