/**
 * The picture at the top of `/about/` as data, seen from behind and a little
 * to the side: a father in his office chair pulled up to the desk, his young
 * son on his lap, his daughter standing beside the chair, all three facing
 * the monitor, which lights them. There is no React in this file:
 * `picture-svg.ts` turns it into SVG, and the animation reads the same shapes
 * back from that SVG.
 *
 * Units are the SVG's user units, about 290 to the metre at the chair, with
 * the floor at y = 557.
 *
 * - The figures are drawn back to front: the father's hand on the mouse,
 *   the son on his lap, the father, the daughter and her braids. Each one
 *   after the first gets a dark gap around it (`halo`), so it reads in front
 *   of what is behind it.
 * - The desk, the chair and the monitor are thin lines; the backrest is a
 *   mesh that dims the father's back behind it.
 */
import { VIEW } from "./picture-rain.ts";

export { VIEW };

export type Point = readonly [x: number, y: number];
/** A joint of a limb: centre and radius. */
type Joint = readonly [x: number, y: number, r: number];

export type Layer = {
  id: "father" | "father-hand" | "navraj" | "navreet" | "braids";
  /** The dark gap drawn around the layer, in user units (0 for the one at the back). */
  halo: number;
  /** Closed paths, filled. */
  pieces: string[];
};

const r1 = (n: number) => Math.round(n * 10) / 10;

/** A circle as a closed path. */
function circle(cx: number, cy: number, r: number): string {
  return ellipse(cx, cy, r, r);
}

/** An ellipse as a closed path: two half arcs. */
function ellipse(cx: number, cy: number, rx: number, ry: number): string {
  return `M${r1(cx + rx)} ${r1(cy)}A${r1(rx)} ${r1(ry)} 0 1 1 ${r1(cx - rx)} ${r1(cy)}A${r1(rx)} ${r1(ry)} 0 1 1 ${r1(cx + rx)} ${r1(cy)}Z`;
}

/**
 * A tapered limb through joints, as pieces: a circle at each joint and the
 * band between each pair (the outer tangents of the two circles). Filled
 * together they make one smooth limb.
 */
function limb(...joints: Joint[]): string[] {
  const out: string[] = joints.map(([x, y, r]) => circle(x, y, r));
  for (let i = 1; i < joints.length; i++) {
    const [ax, ay, ar] = joints[i - 1];
    const [bx, by, br] = joints[i];
    const d = Math.hypot(bx - ax, by - ay);
    if (d <= Math.abs(ar - br)) continue;
    const theta = Math.atan2(by - ay, bx - ax);
    const alpha = Math.acos((ar - br) / d);
    const at = (x: number, y: number, r: number, a: number): Point => [x + r * Math.cos(a), y + r * Math.sin(a)];
    const p = [at(ax, ay, ar, theta + alpha), at(bx, by, br, theta + alpha), at(bx, by, br, theta - alpha), at(ax, ay, ar, theta - alpha)];
    out.push(`M${p.map(([x, y]) => `${r1(x)} ${r1(y)}`).join("L")}Z`);
  }
  return out;
}

/**
 * A braid from `top` to `end`: lobes that shrink from `r0` to `r1` and step
 * a little to each side in turn, so the outline is scalloped like a plait,
 * then a tie and a small tuft.
 */
function braid(top: Point, end: Point, r0: number, r1n: number): string[] {
  const [ax, ay] = top;
  const [bx, by] = end;
  const length = Math.hypot(bx - ax, by - ay);
  const n = Math.max(3, Math.round(length / (r0 * 1.15)));
  const nx = -(by - ay) / length;
  const ny = (bx - ax) / length;
  const out: string[] = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const r = r0 + (r1n - r0) * t;
    const side = (i % 2 ? 1 : -1) * r * 0.28;
    out.push(ellipse(ax + (bx - ax) * t + nx * side, ay + (by - ay) * t + ny * side, r, r * 0.95));
  }
  const ux = (bx - ax) / length;
  const uy = (by - ay) / length;
  out.push(ellipse(bx + ux * r1n * 1.3, by + uy * r1n * 1.3, r1n * 0.8, r1n * 0.55));
  out.push(...limb([bx + ux * r1n * 1.8, by + uy * r1n * 1.8, r1n * 0.85], [bx + ux * r1n * 3.6, by + uy * r1n * 3.6, r1n * 0.35]));
  return out;
}

/**
 * Navraj: a small boy on his father's lap, seen from behind with his head
 * turned to the screen, showing beside his father's right shoulder. His hair
 * is tied in a joora on his crown under a small white cloth (`cloth`).
 */
const navraj: Layer = {
  id: "navraj",
  halo: 2,
  pieces: [
    // Head from behind, turned to the screen: the back of the skull and the edge of the cheek.
    "M510 270C508 256 515 245 527 242C540 239 551 246 554 258C556 266 556 272 553 278" +
      "C551 284 546 289 539 291L537 295C545 298 551 304 553 314C556 326 556 346 554 364" +
      "C553 376 547 384 538 386L506 386C500 380 498 360 499 330C500 312 504 300 512 292C510 286 509 278 510 270Z",
    // The joora on his crown and the band of the cloth around his head.
    ellipse(529, 236, 12.5, 10),
    ellipse(530, 245, 17.5, 4.5),
  ],
};

/** The white cloth over Navraj's joora, drawn paler than the rest of him. */
export const cloth: string[] = [ellipse(529, 236, 12.5, 10), ellipse(530, 245, 17.5, 4.5)];

/**
 * The father, seen from behind and a little to the side: short thick hair,
 * the edge of a full beard on his jaw, broad shoulders over the backrest of
 * his chair, his right hand on the mouse.
 */
const father: Layer = {
  id: "father",
  halo: 2.4,
  pieces: [
    // Head, shoulders and upper arms, down to the top of the backrest and the elbows beside it.
    "M425 252C416 243 412 228 413 212C414 194 423 182 436 178C442 175 448 176 452 175" +
      "C459 175 465 180 468 187C471 193 472 201 471 208C475 209 477 215 475 221" +
      "C480 226 484 236 482 246C480 253 472 258 464 260L462 266C476 270 496 272 508 278" +
      "C517 283 521 296 520 312C521 334 519 356 515 372C513 380 503 382 499 376L501 304" +
      "C482 293 466 289 446 289C426 289 408 293 391 300L389 372" +
      "C386 380 376 382 372 376C368 356 365 334 366 312C366 296 371 284 381 278" +
      "C393 272 412 270 424 266Z",
    // His back, down to the seat, seen through the mesh of the backrest.
    "M394 288L496 288C499 330 500 380 497 438L393 438C390 380 391 330 394 288Z",
  ],
};

/** The father's right hand on the mouse, reaching past his son: behind the boy, so it shows beyond him. */
const fatherHand: Layer = {
  id: "father-hand",
  halo: 0,
  pieces: [...limb([530, 362, 10], [616, 345, 8]), ...limb([616, 345, 8], [634, 339, 7])],
};

/** Navreet: standing beside her father's chair at the desk, seen from behind, a hand on the desk's edge. */
const navreet: Layer = {
  id: "navreet",
  halo: 2.4,
  pieces: [
    // Head from behind, turned a little towards the screen.
    "M284 262C280 250 282 236 291 228C299 221 312 220 321 225C329 230 332 240 331 250" +
      "C333 254 333 258 330 262C328 268 322 273 314 275L296 275C290 273 286 268 284 262Z",
    // Neck, shoulders and back to the waist.
    "M296 272L314 272L316 284C326 286 333 292 334 302C336 320 334 342 330 360L280 360" +
      "C276 342 274 320 276 302C277 292 284 286 294 284Z",
    // Her frock.
    "M281 354L329 354C336 376 342 402 345 428C330 434 280 434 266 428C269 402 274 376 281 354Z",
    // Legs and shoes.
    ...limb([294, 424, 8.5], [293, 546, 7]),
    ...limb([316, 424, 8.5], [318, 546, 7]),
    "M282 546C284 541 300 540 304 546L305 557L280 557C280 552 281 549 282 546Z",
    "M308 546C312 541 326 541 330 546L331 557L307 557Z",
    // Her left hand on the edge of the desk; her right arm at her side.
    ...limb([281, 296, 8], [268, 328, 6.5], [262, 349, 5.5]),
    ...limb([331, 296, 8], [338, 332, 6.5], [340, 360, 5.5]),
  ],
};

/** Navreet's two braids, down her back: a layer of their own, so a thin gap draws them over her. */
const braids: Layer = {
  id: "braids",
  halo: 1.8,
  pieces: [...braid([294, 266], [287, 350], 6.2, 4.2), ...braid([316, 266], [322, 348], 6.2, 4.2)],
};

/** Back to front. */
export const layers: Layer[] = [fatherHand, navraj, father, navreet, braids];

/** A line of the setting: a path, its width (CSS pixels) and how bright (0 to 1). */
export type Stroke = { d: string; w: number; tone: number };

/** The desk, what is on it and the chair below the backrest, as thin lines. */
export const furniture: Stroke[] = [
  // Desk: its top in perspective, the front edge, a leg and a unit of drawers.
  { d: "M205 318L805 318L842 350L168 350Z", w: 1.4, tone: 0.7 },
  { d: "M168 350L168 360L842 360L842 350", w: 1.3, tone: 0.55 },
  { d: "M176 360L176 557M188 360L188 557", w: 1.2, tone: 0.4 },
  { d: "M730 360L730 557L834 557L834 360", w: 1.3, tone: 0.45 },
  { d: "M730 426L834 426M730 493L834 493", w: 1.2, tone: 0.35 },
  { d: "M770 392L794 392M770 459L794 459M770 526L794 526", w: 1.5, tone: 0.45 },
  // Keyboard and mouse.
  { d: "M510 331L616 331L621 342L505 342Z", w: 1.2, tone: 0.65 },
  { d: "M624 342C624 336 628 334 633 334C638 334 642 336 642 342Z", w: 1.2, tone: 0.6 },
  // A small plant and a mug.
  { d: "M214 300L238 300L235 318L217 318Z", w: 1.2, tone: 0.5 },
  { d: "M226 300C222 288 214 283 205 282M226 300C228 287 236 280 246 278M226 300C225 290 226 281 229 272", w: 1.2, tone: 0.45 },
  { d: "M770 300L770 328L792 328L792 300ZM792 306C800 306 800 320 792 320", w: 1.3, tone: 0.5 },
  // The chair below the backrest: its stem, the seat, the gas lift and the base.
  { d: "M436 421L436 440M454 421L454 440", w: 1.3, tone: 0.5 },
  { d: "M384 440L506 440C510 440 512 444 510 449L380 449C378 444 380 440 384 440Z", w: 1.4, tone: 0.55 },
  { d: "M440 449L440 504M450 449L450 504", w: 1.3, tone: 0.45 },
  { d: "M445 506L366 530M445 506L524 530M445 506L420 541M445 506L474 541", w: 1.4, tone: 0.5 },
  { d: circle(366, 545, 7), w: 1.2, tone: 0.45 },
  { d: circle(524, 545, 7), w: 1.2, tone: 0.45 },
  { d: circle(420, 551, 5.5), w: 1.2, tone: 0.4 },
  { d: circle(474, 551, 5.5), w: 1.2, tone: 0.4 },
];

/**
 * The chair's backrest, a mesh over the father's back: drawn over him, it
 * lets `veil` of his digits and glow through (0 hides nothing, 1 all).
 */
export const backrest = {
  d: "M398 298C420 292 466 292 490 298C496 300 498 306 497 314L492 404C491 414 486 420 476 421L414 421C404 420 399 414 398 404L393 314C392 306 393 300 398 298Z",
  mesh: "M400 326L490 326M401 352L489 352M402 378L488 378M403 404L487 404",
  veil: 0.72,
};

/** The monitor, facing the family: the screen's corners (top left, top right, bottom right, bottom left), stand and base. */
export const monitor = {
  screen: [
    [498, 150],
    [710, 145],
    [710, 285],
    [498, 287],
  ] as Point[],
  stand: "M598 287L596 316M610 286L612 316",
  base: "M570 322C570 317 584 315 604 315C624 315 638 317 638 322Z",
};

/** A point on the screen at (u, v), 0 to 1 across and down. */
export function onScreen(u: number, v: number): Point {
  const [tl, tr, br, bl] = monitor.screen;
  const top: Point = [tl[0] + (tr[0] - tl[0]) * u, tl[1] + (tr[1] - tl[1]) * u];
  const bottom: Point = [bl[0] + (br[0] - bl[0]) * u, bl[1] + (br[1] - bl[1]) * u];
  return [top[0] + (bottom[0] - top[0]) * v, top[1] + (bottom[1] - top[1]) * v];
}

/** Where the small Navree mark sits on the screen (its top left, in user units) and how big it is. */
export const screenMark = { at: onScreen(0.055, 0.075), size: 15 } as const;

/** Lines of code on the screen, below the mark: indent, length and brightness (0 to 1); a zero length is a blank line. */
const codeLines: [indent: number, length: number, tone: number][] = [
  [0, 0.4, 0.95],
  [1, 0.5, 0.6],
  [1, 0.34, 0.6],
  [2, 0.44, 0.45],
  [2, 0.28, 0.45],
  [1, 0.2, 0.6],
  [0, 0.08, 0.95],
  [0, 0, 0],
  [0, 0.48, 0.95],
  [1, 0.38, 0.6],
  [2, 0.46, 0.45],
  [1, 0.14, 0.6],
  [0, 0.08, 0.95],
];

/** The screen's text as straight segments, each with its width and brightness: a name beside the mark, then the code. */
export function screenCode(): { from: Point; to: Point; w: number; tone: number }[] {
  const out: { from: Point; to: Point; w: number; tone: number }[] = [];
  const width = (v: number) => r1(Math.max(2, (onScreen(0, v + 0.01)[1] - onScreen(0, v)[1]) * 2.2));
  out.push({ from: onScreen(0.15, 0.115), to: onScreen(0.36, 0.115), w: width(0.115), tone: 0.8 });
  const top = 0.24;
  const step = 0.7 / codeLines.length;
  codeLines.forEach(([indent, length, tone], i) => {
    if (!length) return;
    const v = top + i * step + step / 2;
    const u0 = 0.08 + indent * 0.07;
    out.push({ from: onScreen(u0, v), to: onScreen(Math.min(0.92, u0 + length), v), w: width(v), tone });
  });
  return out;
}

/** A faint column of digits behind the family: where it starts, its digits (top to bottom), spacing and size. */
export type RainColumn = { x: number; y: number; step: number; font: number; text: string };

/** The faint rain in the room, drawn once. */
export function backgroundRain(next: () => number): RainColumn[] {
  const out: RainColumn[] = [];
  for (let x = VIEW.x + 12; x < VIEW.x + VIEW.width - 6; x += 20 + next() * 24) {
    const font = 7.5 + next() * 2.5;
    const step = font * 1.2;
    const length = 5 + Math.floor(next() * 14);
    const y = VIEW.y + next() * (VIEW.height - length * step * 0.7);
    const text = Array.from({ length }, () => (next() < 0.5 ? "0" : "1")).join("");
    out.push({ x: r1(x), y: r1(y), step: r1(step), font: r1(font), text });
  }
  return out;
}

// ---------------------------------------------------------------- inside

/** A path of this file (M, L, C, A and Z, absolute) as polygons, for testing which cells the figures cover. */
function polygons(d: string): Point[][] {
  const tokens = d.match(/[MLCAZ]|-?\d*\.?\d+/g) ?? [];
  const out: Point[][] = [];
  let poly: Point[] = [];
  let i = 0;
  let cmd = "";
  let pos: Point = [0, 0];
  const num = () => Number(tokens[i++]);
  while (i < tokens.length) {
    if (/[MLCAZ]/.test(tokens[i])) cmd = tokens[i++];
    if (cmd === "M") {
      if (poly.length) out.push(poly);
      pos = [num(), num()];
      poly = [pos];
      cmd = "L";
    } else if (cmd === "L") {
      pos = [num(), num()];
      poly.push(pos);
    } else if (cmd === "C") {
      const [x1, y1, x2, y2, x, y] = [num(), num(), num(), num(), num(), num()];
      const [x0, y0] = pos;
      for (let s = 1; s <= 8; s++) {
        const t = s / 8;
        const u = 1 - t;
        poly.push([u * u * u * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x, u * u * u * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y]);
      }
      pos = [x, y];
    } else if (cmd === "A") {
      // Only the half ellipses `ellipse` writes: from one end of the horizontal axis to the other.
      const [rx, ry] = [num(), num()];
      i += 3;
      const [x, y] = [num(), num()];
      const cx = (pos[0] + x) / 2;
      const start = pos[0] > x ? 0 : Math.PI;
      for (let s = 1; s <= 10; s++) {
        const a = start + (Math.PI * s) / 10;
        poly.push([cx + rx * Math.cos(a), y + ry * Math.sin(a)]);
      }
      pos = [x, y];
    } else if (cmd === "Z") {
      if (poly.length) out.push(poly);
      poly = [];
      cmd = "";
    } else {
      i++;
    }
  }
  if (poly.length) out.push(poly);
  return out;
}

function inPolygon([x, y]: Point, poly: Point[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i];
    const [xj, yj] = poly[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

let figurePolygons: Point[][] | null = null;

/** True when (x, y) is inside one of the figures. */
export function insideFigures(x: number, y: number): boolean {
  figurePolygons ??= layers.flatMap((layer) => layer.pieces.flatMap(polygons));
  return figurePolygons.some((poly) => inPolygon([x, y], poly));
}
