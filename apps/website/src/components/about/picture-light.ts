/**
 * The monitor's light, shared by the still picture (as gradient stops) and
 * the animation (per digit), so both shade the figures the same way.
 */

/** Where the light comes from (the middle of the screen), and how far it reaches. */
export const LIGHT = { x: 604, y: 216, reach: 520, squash: 1.2 } as const;

const FLOOR = 0.5;
const CURVE = 1.3;

/** How much of the monitor's light reaches a point: 0.5 (far) to 1 (at the screen). */
export function lightAt(x: number, y: number): number {
  const d = Math.hypot(x - LIGHT.x, (y - LIGHT.y) * LIGHT.squash);
  const t = Math.max(0, 1 - d / LIGHT.reach);
  return FLOOR + (1 - FLOOR) * t ** CURVE;
}

/** The light as gradient stops: offset 0 to 1 of the reach, and the brightness there. */
export function lightStops(n = 8): { offset: number; value: number }[] {
  return Array.from({ length: n + 1 }, (_, i) => {
    const offset = i / n;
    return { offset, value: FLOOR + (1 - FLOOR) * (1 - offset) ** CURVE };
  });
}
