/**
 * Builds the live picture's glyphs, once, when the scene starts: where each
 * one sits, how bright it is, and what it does over time. The result is
 * plain typed arrays, one row per glyph, that the scene hands to the GPU as
 * instance attributes; nothing here changes per frame.
 *
 * - The picture's digits: a grid laid over the picture, one digit wherever
 *   the picture has light, as bright as the picture is there. In the
 *   opening they rain down and land, and the picture appears under them;
 *   then, over the family, they shimmer and stream (elsewhere they rest).
 * - Dust: a finer, smaller fall of light with them in the opening (wide
 *   frames only).
 * - The rain at the picture's left and right edges, in place of the
 *   picture's own.
 * - The code typed on the screen, its line numbers, and the caret.
 * - A soft glow over the screen, and a few out-of-focus digits near the
 *   camera.
 *
 * Also draws the mask the picture's shader reads: the screen's glass
 * (where the code goes, minus Navreet's head) and the edge rain zones.
 */
import {
  CODE,
  CODE_LAYOUT,
  CODE_TITLE,
  HEADS,
  HEAD_OVER_SCREEN,
  IMAGE,
  LIGHT,
  PATKA,
  QUIET,
  RAIN_ZONES,
  SCREEN,
  codeBaseline,
  codeX,
  type Point,
} from "../picture.ts";
import { CELL, DIGIT, TEXT, charCell } from "./atlas.ts";
import type { DepthMap } from "./depth.ts";
import { scaleAt, toWorld } from "./space.ts";
import { schedule, type Schedule } from "./typing.ts";

/** What a glyph is: the shader animates each kind its own way. */
export const GROUP = { picture: 0, dust: 1, rain: 2, code: 3, caret: 4, glow: 5, bokeh: 6 } as const;

/** How fine the picture is drawn: the wide frame of a desktop, or the narrower one of a phone. */
export type Tier = {
  name: "wide" | "narrow";
  /** The grid of the picture's digits: column width and row height (picture pixels). */
  cell: readonly [number, number];
  /** How tall those digits stand. */
  glyph: number;
  /** A finer fall of light in the opening. */
  dust: boolean;
  /** The edge rain: column spacing, row height, digit height. */
  rain: readonly [number, number, number];
  /** Out-of-focus digits near the camera. */
  bokeh: number;
  /** The picture's mesh: quads across. */
  mesh: number;
};

export const TIERS: Record<Tier["name"], Tier> = {
  wide: { name: "wide", cell: [9, 12], glyph: 9.5, dust: true, rain: [26, 18, 13.5], bokeh: 16, mesh: 176 },
  narrow: { name: "narrow", cell: [13, 17], glyph: 13, dust: false, rain: [30, 22, 16], bokeh: 6, mesh: 96 },
};

export type Layout = {
  count: number;
  /** World x, y, z and size (the height of the glyph's quad). */
  a: Float32Array;
  /** Atlas cell (-1 for a digit that changes), brightness, tint towards mint, a value of the group's own. */
  b: Float32Array;
  /** Seed (0 to 1), group, glow, a value of the group's own. */
  c: Float32Array;
  /** Picture x and y (for the landing time), column, row or flag. */
  d: Float32Array;
  /** How many glyphs to draw at each step down in quality: all; without the dust; with half the opening's rain gone too. */
  steps: number[];
  /** How many glyphs can still show once the opening is over (they come first: the opening's rain and dust follow). */
  essential: number;
  /** The mask the picture's shader reads: red the screen's glass, green the edge rain zones, blue Navraj's patka. */
  mask: HTMLCanvasElement;
  /** The light: the middle of the screen, in the world. */
  light: [number, number, number];
  typing: Schedule;
  /** The world position of the caret before line `line`, column `col`. */
  caretAt: (line: number, col: number) => [number, number, number];
};

/** Rows of instance data, and their order. */
class Rows {
  a: number[] = [];
  b: number[] = [];
  c: number[] = [];
  d: number[] = [];
  get length() {
    return this.c.length / 4;
  }
  push(a: number[], b: number[], c: number[], d: number[]) {
    this.a.push(...a);
    this.b.push(...b);
    this.c.push(...c);
    this.d.push(...d);
  }
  /** Shuffles the rows, so that drawing only the first part of them still covers the whole picture. */
  shuffle(next: () => number) {
    const n = this.length;
    const order = Array.from({ length: n }, (_, i) => i);
    for (let i = n - 1; i > 0; i--) {
      const j = Math.floor(next() * (i + 1));
      [order[i], order[j]] = [order[j], order[i]];
    }
    for (const key of ["a", "b", "c", "d"] as const) {
      const src = this[key];
      this[key] = order.flatMap((i) => src.slice(i * 4, i * 4 + 4));
    }
  }
}

/** A seeded generator (mulberry32), so every visit builds the same picture. */
function random(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const path = (ctx: CanvasRenderingContext2D, poly: readonly Point[]) => {
  ctx.beginPath();
  poly.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
  ctx.closePath();
};

/** The picture's mask at half size: red is the glass (minus Navreet's head), green the edge rain zones, blue the patka, feathered. */
function drawMask(): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = IMAGE.width / 2;
  canvas.height = IMAGE.height / 2;
  const ctx = canvas.getContext("2d");
  if (!ctx) return canvas;
  ctx.fillStyle = "#000";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.scale(0.5, 0.5);
  ctx.globalCompositeOperation = "lighter";
  ctx.filter = "blur(1px)";
  ctx.fillStyle = "#f00";
  ctx.fillRect(SCREEN.x, SCREEN.y, SCREEN.width, SCREEN.height);
  ctx.globalCompositeOperation = "source-over";
  ctx.fillStyle = "#000";
  ctx.filter = "blur(1.5px)";
  path(ctx, HEAD_OVER_SCREEN);
  ctx.fill();
  ctx.globalCompositeOperation = "lighter";
  ctx.filter = "blur(8px)";
  ctx.fillStyle = "#0f0";
  for (const zone of RAIN_ZONES) {
    path(ctx, zone);
    ctx.fill();
  }
  ctx.filter = "blur(1.5px)";
  ctx.fillStyle = "#00f";
  path(ctx, PATKA);
  ctx.fill();
  ctx.filter = "none";
  return canvas;
}

/** Builds every glyph of the picture for a tier from the picture itself (`image`, already decoded) and its depth. */
export async function buildLayout(
  tier: Tier,
  image: CanvasImageSource,
  depth: DepthMap,
  pause: () => Promise<void>,
): Promise<Layout> {
  const next = random(tier.name === "wide" ? 4242 : 2424);
  const essential = new Rows();
  const rain = new Rows();
  const dust = new Rows();
  const mask = drawMask();
  /** A test for whether a picture pixel lies in any of `polys`, from a quarter-size drawing of them. */
  const within = (polys: readonly (readonly Point[])[]) => {
    const c = document.createElement("canvas");
    c.width = IMAGE.width / 4;
    c.height = IMAGE.height / 4;
    const ctx = c.getContext("2d", { willReadFrequently: true });
    if (!ctx) return () => false;
    ctx.scale(0.25, 0.25);
    ctx.fillStyle = "#fff";
    for (const poly of polys) {
      path(ctx, poly);
      ctx.fill();
    }
    const data = ctx.getImageData(0, 0, c.width, c.height).data;
    c.width = c.height = 0;
    return (x: number, y: number) => {
      const px = Math.min(IMAGE.width / 4 - 1, Math.max(0, Math.floor(x / 4)));
      const py = Math.min(IMAGE.height / 4 - 1, Math.max(0, Math.floor(y / 4)));
      return data[(py * (IMAGE.width / 4) + px) * 4 + 3] > 127;
    };
  };
  const zones = within(RAIN_ZONES);
  const quiet = within(QUIET);
  const patka = within([PATKA]);
  const box = PATKA.reduce((b, [x, y]) => [Math.min(b[0], x), Math.min(b[1], y), Math.max(b[2], x), Math.max(b[3], y)], [Infinity, Infinity, -Infinity, -Infinity]);
  const near = (x: number, y: number) => x > box[0] - 16 && x < box[2] + 16 && y > box[1] - 16 && y < box[3] + 16;

  /** The picture's light averaged over a grid of `cw` x `ch` cells: luminance and whiteness per cell. */
  const sampleGrid = (cw: number, ch: number) => {
    const cols = Math.floor(IMAGE.width / cw);
    const rows = Math.floor(IMAGE.height / ch);
    const c = document.createElement("canvas");
    c.width = cols;
    c.height = rows;
    const ctx = c.getContext("2d", { willReadFrequently: true });
    if (!ctx) return { cols: 0, rows: 0, light: new Float32Array(0), white: new Float32Array(0) };
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(image, 0, 0, cols, rows);
    const px = ctx.getImageData(0, 0, cols, rows).data;
    const light = new Float32Array(cols * rows);
    const white = new Float32Array(cols * rows);
    for (let i = 0; i < light.length; i++) {
      const r = px[i * 4] / 255;
      const g = px[i * 4 + 1] / 255;
      const b = px[i * 4 + 2] / 255;
      light[i] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      white[i] = Math.max(r, g, b) > 0.05 ? Math.min(r, g, b) / Math.max(r, g, b) : 0;
    }
    c.width = c.height = 0;
    return { cols, rows, light, white };
  };

  // ---- the picture's digits
  {
    const [cw, ch] = tier.cell;
    const grid = sampleGrid(cw, ch);
    for (let row = 0; row < grid.rows; row++) {
      for (let col = 0; col < grid.cols; col++) {
        const i = row * grid.cols + col;
        const l = grid.light[i];
        if (l < 0.07) continue;
        const x = (col + 0.5) * cw;
        const y = (row + 0.5) * ch;
        if (zones(x, y) || patka(x, y)) continue;
        const z = depth.at(x, y) - 3;
        const figure = depth.isFigure(x, y) && !quiet(x, y) ? 1 : 0;
        essential.push(
          [...toWorld(x, y, z), (tier.glyph / DIGIT) * scaleAt(z)],
          [-1, Math.min(1.1, 0.25 + l * 1.5), Math.min(1, grid.white[i] * 1.3), figure],
          [next(), GROUP.picture, 0.5, 0],
          [x, y, col + 1000 * figure, row],
        );
      }
    }
  }

  await pause();

  // ---- dust: a finer fall of light, in the opening only
  if (tier.dust) {
    const [cw, ch] = [tier.cell[0] / 2, tier.cell[1] / 2];
    const grid = sampleGrid(cw, ch);
    for (let row = 0; row < grid.rows; row++) {
      for (let col = 0; col < grid.cols; col++) {
        const l = grid.light[row * grid.cols + col];
        if (l < 0.1 || next() > 0.75) continue;
        const x = (col + 0.5) * cw + (next() - 0.5) * cw * 0.6;
        const y = (row + 0.5) * ch;
        if (zones(x, y) || patka(x, y)) continue;
        const z = depth.at(x, y) - 4;
        dust.push([...toWorld(x, y, z), 7 * scaleAt(z)], [CELL.glow, Math.min(1, l * 1.4), 0.4, 0], [next(), GROUP.dust, 0, 0], [x, y, 0, 0]);
      }
    }
  }

  await pause();

  // ---- the rain: at the edges, where the picture has its own, and in the opening across the whole room
  {
    const [rw, rh, height] = tier.rain;
    const z = 650;
    let column = 0;
    for (let x = rw * 0.4; x < IMAGE.width; x += rw, column++) {
      let row = 0;
      for (let y = 6; y < IMAGE.height; y += rh, row++) {
        const edge = zones(x, y);
        // The opening's rain falls past the patka, never down it.
        if (!edge && near(x, y)) continue;
        // Fainter towards the bottom, as in the picture.
        const fade = Math.max(0.15, 1 - y / 520);
        (edge ? essential : rain).push(
          [...toWorld(x, y, z), (height / DIGIT) * scaleAt(z)],
          [-1, edge ? fade : 0.55, 0.05, edge ? 1 : 0],
          [next(), GROUP.rain, 0.55, 0],
          [x, y, 9000 + column, row],
        );
      }
    }
  }

  await pause();

  // ---- the code on the screen
  const typing = schedule(CODE);
  const screenZ = depth.at(SCREEN.x + SCREEN.width / 2, SCREEN.y + SCREEN.height / 2) - 2;
  const cap = CODE_LAYOUT.size * 0.7;
  const advance = CODE_LAYOUT.size * CODE_LAYOUT.advance;
  const textSize = (cap / TEXT) * scaleAt(screenZ);
  const charAt = (x: number, baseline: number) => toWorld(x + advance / 2, baseline - cap / 2, screenZ);
  const codeGlyph = (cell: number, p: [number, number, number], tone: number, appear: number, tint = 0.3) => {
    essential.push([...p, textSize], [cell, tone, tint, 0], [next(), GROUP.code, 0.55, appear], [0, 0, 0, 0]);
  };
  for (let i = 0; i < CODE_TITLE.length; i++) {
    if (CODE_TITLE[i] !== " ") codeGlyph(charCell(CODE_TITLE[i]), charAt(codeX(i), CODE_LAYOUT.title), 0.5, -1e3);
  }
  CODE.forEach((_, i) => {
    const n = String(i + 1);
    for (let k = 0; k < n.length; k++) {
      const x = CODE_LAYOUT.gutter - (n.length - k) * advance;
      codeGlyph(charCell(n[k]), charAt(x, codeBaseline(i)), 0.32, -1e3, 0);
    }
  });
  for (const c of typing.chars) {
    const line = CODE[c.line];
    const tone = line.startsWith("#[") || line.startsWith("    #[") ? 0.95 : line.startsWith("        ") ? 0.7 : 0.82;
    const tint = line.trimStart().startsWith("#[") ? 0.55 : 0.2;
    codeGlyph(charCell(c.ch), charAt(codeX(c.col), codeBaseline(c.line)), tone, c.at, tint);
  }
  const caretAt = (line: number, col: number) => charAt(codeX(col), line < 0 ? CODE_LAYOUT.title : codeBaseline(line));
  essential.push([...caretAt(0, 0), textSize], [CELL.caret, 0.9, 0.5, 0], [0.5, GROUP.caret, 0.6, 0], [0, 0, 0, 0]);

  // ---- a soft glow on the screen, which flashes with each pulse
  {
    const p = toWorld(LIGHT.x, LIGHT.y, screenZ - 4);
    essential.push([...p, 330 * scaleAt(screenZ)], [CELL.glow, 0.14, 0.3, 0], [0.5, GROUP.glow, 1, 1.5], [LIGHT.x, LIGHT.y, 0, 0]);
  }

  // ---- out-of-focus digits near the camera, away from the family's heads and the screen
  let bokeh = tier.bokeh;
  for (let i = 0; i < tier.bokeh * 6 && bokeh > 0; i++) {
    const x = 30 + next() * (IMAGE.width - 60);
    const y = next() * IMAGE.height;
    if (HEADS.some(([hx, hy]) => Math.hypot(hx - x, hy - y) < 170)) continue;
    if (x > SCREEN.x - 30 && x < SCREEN.x + SCREEN.width + 30 && y < SCREEN.y + SCREEN.height + 30) continue;
    if (x > 330 && x < 1100) continue;
    const z = -700 - next() * 300;
    const height = 30 + next() * 50;
    essential.push(
      [...toWorld(x, y, z), (height / DIGIT) * scaleAt(z)],
      [-1, 0.05 + 0.06 * next(), 0.2, 0],
      [next(), GROUP.bokeh, 0, 10 + next() * 16],
      [x, y, 0, height],
    );
    bokeh--;
  }

  await pause();

  rain.shuffle(next);
  dust.shuffle(next);
  const count = essential.length + rain.length + dust.length;
  const pack = (key: "a" | "b" | "c" | "d") => {
    const out = new Float32Array(count * 4);
    out.set(essential[key], 0);
    out.set(rain[key], essential.length * 4);
    out.set(dust[key], (essential.length + rain.length) * 4);
    return out;
  };

  return {
    count,
    a: pack("a"),
    b: pack("b"),
    c: pack("c"),
    d: pack("d"),
    steps: [count, essential.length + rain.length, essential.length + Math.round(rain.length / 2)],
    essential: essential.length,
    mask,
    light: toWorld(LIGHT.x, LIGHT.y, LIGHT.z),
    typing,
    caretAt,
  };
}
