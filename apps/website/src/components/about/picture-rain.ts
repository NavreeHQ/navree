/**
 * The digital rain inside the figures, shared by the still picture (drawn on
 * the server) and the animation (`family-rain.tsx`): the grid of cells, the
 * digit in each cell, the drops falling down the columns and how bright
 * each cell is. Everything comes from seeded generators, so the still and
 * the first frame of the animation are the same picture.
 *
 * Units are the picture's user units (see `picture-model.ts`).
 */

/** The picture's view box, in user units (about 300 to the metre). */
export const VIEW = { x: 150, y: 112, width: 712, height: 462 } as const;

/** A grid of digit cells: its origin, its cell size, the font size, and how many columns and rows. */
export type Grid = {
  name: "wide" | "narrow";
  x: number;
  y: number;
  cw: number;
  ch: number;
  font: number;
  cols: number;
  rows: number;
  seed: number;
};

/** The area the figures cover, which the grids fill. */
export const FIGURES_BOX = { x: 255, y: 160, right: 650, bottom: 560 } as const;

function grid(name: Grid["name"], cw: number, ch: number, font: number, seed: number): Grid {
  const cols = Math.ceil((FIGURES_BOX.right - FIGURES_BOX.x) / cw);
  const rows = Math.ceil((FIGURES_BOX.bottom - FIGURES_BOX.y) / ch);
  return { name, x: FIGURES_BOX.x, y: FIGURES_BOX.y, cw, ch, font, cols, rows, seed };
}

/**
 * Two grids: fine digits where the picture is wide, bigger ones on phones,
 * where it is drawn at about half the size. The page shows one or the other
 * from a width of 40rem.
 */
export const grids: Grid[] = [grid("wide", 7.5, 9.5, 8.6, 1517), grid("narrow", 11.5, 14, 13, 2011)];

/** The width of the picture, in CSS pixels, below which the narrow grid is used (40rem, as the page's container query). */
export const NARROW_BELOW = 640;

/** A seeded generator (mulberry32). */
export function random(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** The digit of every cell (row by row, `"0"` or `"1"`), and a dim or plain base level for each. */
export function cells(g: Grid): { chars: string[]; base: Uint8Array } {
  const next = random(g.seed);
  const n = g.cols * g.rows;
  const chars: string[] = [];
  const base = new Uint8Array(n);
  for (let i = 0; i < n; i++) {
    chars.push(next() < 0.5 ? "0" : "1");
    base[i] = next() < 0.35 ? 0 : 1;
  }
  return { chars, base };
}

/** A drop falling down a column: the row of its head (fractional), rows per tick, and its length in cells. */
export type Drop = { col: number; head: number; speed: number; length: number };

/** A new drop for a column, starting above the grid or (for the first frame) anywhere in it. */
export function newDrop(g: Grid, col: number, next: () => number, anywhere: boolean): Drop {
  const length = 5 + Math.floor(next() * (g.name === "wide" ? 16 : 10));
  const head = anywhere ? next() * (g.rows + length) : -next() * g.rows * 0.6;
  return { col, head, speed: 0.34 + next() * 0.5, length };
}

/** The drops of the first frame: about one in every column, at a random height. */
export function firstDrops(g: Grid): Drop[] {
  const next = random(g.seed * 7 + 3);
  const drops: Drop[] = [];
  for (let col = 0; col < g.cols; col++) {
    if (next() < 0.72) drops.push(newDrop(g, col, next, true));
  }
  return drops;
}

/**
 * Opacity of each brightness level: the two base levels, the trail's levels
 * and, last, the head of a drop (drawn in a pale mint instead of green).
 */
export const LEVELS = [0.22, 0.32, 0.46, 0.62, 0.8, 1, 1] as const;
export const HEAD = LEVELS.length - 1;

/** The level a drop gives a cell `d` rows above its head (0 is the head itself), or -1 if it doesn't reach it. */
export function trailLevel(d: number, length: number): number {
  if (d < 0 || d >= length) return -1;
  if (d < 1) return HEAD;
  const t = 1 - d / length;
  return 1 + Math.round(t ** 1.25 * (HEAD - 2));
}

/** The level of every cell for a set of drops. */
export function levels(g: Grid, base: Uint8Array, drops: Drop[]): Uint8Array {
  const out = Uint8Array.from(base);
  for (const drop of drops) {
    const head = Math.floor(drop.head);
    for (let d = 0; d < drop.length; d++) {
      const row = head - d;
      if (row < 0 || row >= g.rows) continue;
      const level = trailLevel(d, drop.length);
      const i = row * g.cols + drop.col;
      if (level > out[i]) out[i] = level;
    }
  }
  return out;
}

/** The centre x of a column and the baseline y of a row, in user units. */
export const cellX = (g: Grid, col: number) => g.x + col * g.cw + g.cw / 2;
export const cellBaseline = (g: Grid, row: number) => g.y + row * g.ch + g.ch * 0.78;
