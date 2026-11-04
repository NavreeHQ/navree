/**
 * The picture's depth, built from the hand-traced layers in `picture.ts`:
 * a small grid holding, for every few pixels of the picture, how far back
 * that part of it stands, and which figure (if any) it belongs to.
 *
 * Each layer is drawn back to front, so a nearer one covers a farther one.
 * A figure swells towards the viewer like a cushion, deepest in the middle,
 * using the distance from each point to the figure's edge (a distance
 * transform over the drawn shape). The result is smoothed a little, so the
 * picture stretches gently at the edges between depths when the camera
 * moves, instead of tearing.
 */
import { DEPTH_LAYERS, IMAGE, ROOM_DEPTH } from "../picture.ts";

export type DepthMap = {
  /** Cells across and down; each cell is `cell` picture pixels square. */
  cols: number;
  rows: number;
  cell: number;
  /** Depth of each cell, row by row. */
  z: Float32Array;
  /** 1 where the cell belongs to a figure (the father, Navraj or Navreet). */
  figure: Uint8Array;
  /** Depth at picture pixel (x, y), interpolated between cells. */
  at(x: number, y: number): number;
  /** Whether picture pixel (x, y) is part of a figure. */
  isFigure(x: number, y: number): boolean;
};

/** One line of the exact Euclidean distance transform (Felzenszwalb and Huttenlocher), on squared distances. */
function edt1(f: Float64Array, n: number, d: Float64Array, v: Int32Array, z: Float64Array) {
  let k = 0;
  v[0] = 0;
  z[0] = -Infinity;
  z[1] = Infinity;
  for (let q = 1; q < n; q++) {
    let s = (f[q] + q * q - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k]);
    while (s <= z[k]) {
      k--;
      s = (f[q] + q * q - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k]);
    }
    k++;
    v[k] = q;
    z[k] = s;
    z[k + 1] = Infinity;
  }
  k = 0;
  for (let q = 0; q < n; q++) {
    while (z[k + 1] < q) k++;
    d[q] = (q - v[k]) * (q - v[k]) + f[v[k]];
  }
}

/** Distance (in cells) from each cell inside `mask` to the nearest cell outside it. */
function distance(mask: Uint8Array, w: number, h: number): Float32Array {
  const n = Math.max(w, h);
  const g = new Float64Array(w * h);
  for (let i = 0; i < g.length; i++) g[i] = mask[i] ? 1e12 : 0;
  const f = new Float64Array(n);
  const d = new Float64Array(n);
  const v = new Int32Array(n);
  const z = new Float64Array(n + 1);
  for (let x = 0; x < w; x++) {
    for (let y = 0; y < h; y++) f[y] = g[y * w + x];
    edt1(f, h, d, v, z);
    for (let y = 0; y < h; y++) g[y * w + x] = d[y];
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) f[x] = g[y * w + x];
    edt1(f, w, d, v, z);
    for (let x = 0; x < w; x++) g[y * w + x] = d[x];
  }
  const out = new Float32Array(w * h);
  for (let i = 0; i < out.length; i++) out[i] = Math.sqrt(g[i]);
  return out;
}

/** A box blur of `r` cells, in place, run twice (close to a gaussian). */
function blur(src: Float32Array, w: number, h: number, r: number) {
  const tmp = new Float32Array(src.length);
  for (let pass = 0; pass < 2; pass++) {
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        let s = 0;
        let n = 0;
        for (let k = -r; k <= r; k++) {
          const xx = Math.min(w - 1, Math.max(0, x + k));
          s += src[y * w + xx];
          n++;
        }
        tmp[y * w + x] = s / n;
      }
    }
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        let s = 0;
        let n = 0;
        for (let k = -r; k <= r; k++) {
          const yy = Math.min(h - 1, Math.max(0, y + k));
          s += tmp[yy * w + x];
          n++;
        }
        src[y * w + x] = s / n;
      }
    }
  }
}

/** Builds the depth map; `pause` lets the page breathe between layers. */
export async function buildDepth(pause: () => Promise<void>, cell = 4): Promise<DepthMap> {
  const cols = Math.ceil(IMAGE.width / cell);
  const rows = Math.ceil(IMAGE.height / cell);
  const canvas = document.createElement("canvas");
  canvas.width = cols;
  canvas.height = rows;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("no 2d context");
  const z = new Float32Array(cols * rows).fill(ROOM_DEPTH);
  const figure = new Uint8Array(cols * rows);

  for (const layer of DEPTH_LAYERS) {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, cols, rows);
    ctx.setTransform(1 / cell, 0, 0, 1 / cell, 0, 0);
    ctx.beginPath();
    layer.poly.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.closePath();
    ctx.fillStyle = "#fff";
    ctx.fill();
    const data = ctx.getImageData(0, 0, cols, rows).data;
    const mask = new Uint8Array(cols * rows);
    for (let i = 0; i < mask.length; i++) mask[i] = data[i * 4 + 3] > 127 ? 1 : 0;
    const round = layer.round ?? 0;
    const dist = round ? distance(mask, cols, rows) : null;
    const [sx, sy] = layer.slope ?? [0, 0];
    const [ax, ay] = layer.at ?? layer.poly[0];
    const reach = Math.max(8, round * 0.9) / cell;
    for (let i = 0; i < mask.length; i++) {
      if (!mask[i]) continue;
      const x = (i % cols) * cell + cell / 2;
      const y = Math.floor(i / cols) * cell + cell / 2;
      let depth = layer.z + sx * (x - ax) + sy * (y - ay);
      if (dist) {
        const u = Math.min(1, dist[i] / reach);
        depth -= round * Math.sqrt(1 - (1 - u) * (1 - u));
      }
      z[i] = depth;
      figure[i] = layer.figure ? 1 : 0;
    }
    if (round) await pause();
  }
  blur(z, cols, rows, 2);
  canvas.width = canvas.height = 0;

  const at = (x: number, y: number) => {
    const fx = Math.min(cols - 1.001, Math.max(0, x / cell - 0.5));
    const fy = Math.min(rows - 1.001, Math.max(0, y / cell - 0.5));
    const x0 = Math.floor(fx);
    const y0 = Math.floor(fy);
    const tx = fx - x0;
    const ty = fy - y0;
    const i = y0 * cols + x0;
    const top = z[i] + (z[i + 1] - z[i]) * tx;
    const bottom = z[i + cols] + (z[i + cols + 1] - z[i + cols]) * tx;
    return top + (bottom - top) * ty;
  };
  const isFigure = (x: number, y: number) => {
    const cx = Math.min(cols - 1, Math.max(0, Math.floor(x / cell)));
    const cy = Math.min(rows - 1, Math.max(0, Math.floor(y / cell)));
    return figure[cy * cols + cx] === 1;
  };
  return { cols, rows, cell, z, figure, at, isFigure };
}
