/**
 * Every beat of the live about picture, in seconds from the moment it
 * starts, and the curves between them. The scene reads its timing from here
 * and nowhere else, so the picture is a pure function of time: frame `t`
 * looks the same however it was reached, which is what lets a capture step
 * through it frame by frame.
 *
 * The opening (0 to about 3 s, once per visit):
 * - 0.0: digital rain is already falling at the edges; nothing has formed.
 * - 0.45 to 2.3: column by column, digits fall, slow down and settle; the
 *   picture appears under them as they land (`LAND` in `space.ts`).
 * - 1.5 to 2.4: the screen comes on.
 * - 1.9 to 2.9: its light reaches the family; the rain in the room calms.
 * - 2.6: the first pulse of light runs from the screen through them.
 * - 0.0 to 3.2: the camera eases in from a little further back and higher.
 *
 * Then it is alive: the code types on the screen from 3.2 s, a pulse runs
 * every 8 s, and digits shimmer and stream gently through the family.
 */

/** Cubic Bézier easing, as CSS writes it: `bezier(x1, y1, x2, y2)(t)`. */
export function bezier(x1: number, y1: number, x2: number, y2: number): (t: number) => number {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const x = (s: number) => ((ax * s + bx) * s + cx) * s;
  const y = (s: number) => ((ay * s + by) * s + cy) * s;
  const dx = (s: number) => (3 * ax * s + 2 * bx) * s + cx;
  return (t: number) => {
    if (t <= 0) return 0;
    if (t >= 1) return 1;
    let s = t;
    for (let i = 0; i < 6; i++) {
      const d = dx(s);
      if (Math.abs(d) < 1e-6) break;
      s -= (x(s) - t) / d;
    }
    return y(Math.min(1, Math.max(0, s)));
  };
}

/** Arrivals: fast in, long gentle settle. */
export const easeOut = bezier(0.16, 1, 0.3, 1);
/** Moves between two states. */
export const easeInOut = bezier(0.65, 0, 0.35, 1);

/** Eased progress of `t` from `t0` to `t1` (0 before, 1 after). */
export function progress(t: number, t0: number, t1: number, ease: (x: number) => number = easeInOut): number {
  return ease((t - t0) / (t1 - t0));
}

export const BEATS = {
  /** The screen comes on. */
  screenOn: [1.5, 2.4],
  /** The screen's light reaches the family; the room's rain calms. */
  lightOn: [1.9, 2.9],
  /** The camera's move in. */
  camera: [0, 3.2],
  /** The opening is over: everything has settled. A visit that has seen it starts here. */
  settled: 3.2,
  /** The code starts typing. */
  typeFrom: 3.2,
  /** The first pulse of light, and the time between pulses. */
  pulse: 2.6,
  pulseEvery: 8,
} as const;

/** How the digits fall in the opening: speed (picture pixels a second) and the stretch over which they slow down. */
export const FALL = { speed: 980, slow: 240 } as const;
/** Seconds the slowing takes: an ease-out cubic whose starting speed matches `FALL.speed`. */
export const SLOW_FOR = (3 * FALL.slow) / FALL.speed;

/** The pulse: how fast its front travels (picture pixels a second), how wide it is, and how long it lasts. */
export const PULSE = { speed: 520, width: 46, life: 2.2 } as const;

/** Typing on the screen: seconds a character, at a line's end and at a blank line; the hold at the end; the clear. */
export const TYPING = { char: 0.05, line: 0.36, blank: 0.7, hold: 3.6, clear: 0.5, rest: 0.7 } as const;

/** The caret blinks this many times a second while nothing is typed. */
export const CARET_HZ = 2.4;

/** The camera's slow breathing once settled: amplitude (picture pixels) and period (seconds) on each axis. */
export const BREATH = { x: [6, 17], y: [3.5, 23] } as const;

/** How strongly the light, the screen and the room's rain stand at time `t`. */
export function levels(t: number) {
  return {
    screen: 0.06 + 0.94 * progress(t, BEATS.screenOn[0], BEATS.screenOn[1], easeOut),
    light: progress(t, BEATS.lightOn[0], BEATS.lightOn[1], easeOut),
    /** Strong while the opening's rain pours, then the calm level of the room. */
    rain: 1.6 - 0.95 * progress(t, BEATS.lightOn[0] - 0.3, BEATS.settled, easeInOut),
  };
}

/** The camera's offset from rest at time `t`, in picture pixels: the move in, then the breathing. */
export function cameraAt(t: number): { x: number; y: number; dolly: number } {
  const k = 1 - progress(t, BEATS.camera[0], BEATS.camera[1], easeInOut);
  const breathe = progress(t, BEATS.settled - 0.6, BEATS.settled + 2.4, easeInOut);
  const tau = Math.PI * 2;
  return {
    x: breathe * BREATH.x[0] * Math.sin((tau * t) / BREATH.x[1]),
    y: k * 40 + breathe * BREATH.y[0] * Math.sin((tau * t) / BREATH.y[1] + 1.3),
    dolly: k * 0.07,
  };
}
