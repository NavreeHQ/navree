/**
 * The live picture's space. Points are given in the picture's own pixels
 * (`x`, `y`, at 1376 x 768) plus a depth `z` (same units, growing away from
 * the viewer). From the resting camera every point lands exactly on the
 * pixel it came from, whatever its depth, so the live scene at rest is the
 * picture; when the camera moves a little, nearer things move more than
 * farther ones.
 */
import { IMAGE } from "../picture.ts";

/** The resting camera's distance from the depth-0 plane (the chair), in picture pixels. */
export const DEPTH = 2000;

/** The middle of the picture, which the camera looks at. */
export const CENTER = { x: IMAGE.width / 2, y: IMAGE.height / 2 } as const;

/** How much larger a thing at depth `z` is in the world, so that it looks the size the picture gives it. */
export const scaleAt = (z: number) => (DEPTH + z) / DEPTH;

/** The world position (three.js axes: y up, the camera looking down -z) of picture pixel (x, y) at depth `z`. */
export function toWorld(x: number, y: number, z: number): [number, number, number] {
  const k = scaleAt(z);
  return [CENTER.x + (x - CENTER.x) * k, -(CENTER.y + (y - CENTER.y) * k), -z];
}

/** The vertical field of view (degrees) that shows the picture's full height from the resting camera. */
export const FOV = (2 * Math.atan(IMAGE.height / 2 / DEPTH) * 180) / Math.PI;

/**
 * The opening: columns of `COLUMN` picture pixels land together, from the
 * top down; the middle of the family first and the edges last. The
 * shaders compute the landing time (`landAt` in `shaders.ts`) for the
 * picture and its digits alike, so each column of the picture appears
 * exactly as its digits land on it.
 */
export const COLUMN = 9;
export const LAND = { from: 0.45, spread: 0.5, edges: 0.45, fall: 0.8, middle: 700, reach: 640 } as const;
