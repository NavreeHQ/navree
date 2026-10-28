/**
 * The spring curve the `motion` library animates with, for components that
 * animate one value with plain requestAnimationFrame or the Web Animations
 * API instead of loading the library. Same physics, same closed-form
 * solution, starting from rest: the motion looks exactly the same.
 */

/**
 * For a spring released at rest, the share of the distance still to go after
 * `ms` milliseconds: 1 at the start, approaching 0. Underdamped springs
 * overshoot (negative values).
 */
export function springRemaining({ stiffness, damping, mass = 1 }: { stiffness: number; damping: number; mass?: number }) {
  const ratio = damping / (2 * Math.sqrt(stiffness * mass));
  const undamped = Math.sqrt(stiffness / mass) / 1000; // per ms
  const decay = ratio * undamped;
  if (ratio < 1) {
    const angular = undamped * Math.sqrt(1 - ratio * ratio);
    return (ms: number) => Math.exp(-decay * ms) * ((decay / angular) * Math.sin(angular * ms) + Math.cos(angular * ms));
  }
  if (ratio === 1) return (ms: number) => Math.exp(-undamped * ms) * (1 + undamped * ms);
  const damped = undamped * Math.sqrt(ratio * ratio - 1);
  const slow = decay - damped;
  const fast = decay + damped;
  return (ms: number) => (fast * Math.exp(-slow * ms) - slow * Math.exp(-fast * ms)) / (fast - slow);
}

/**
 * The spring as keyframe progress (0 to 1) every frame until it is within
 * `rest` of the end, for `element.animate()`: offsets and progress values,
 * and the total duration in ms.
 */
export function springKeyframes(
  physics: { stiffness: number; damping: number; mass?: number },
  { rest = 0.002, frame = 1000 / 60 }: { rest?: number; frame?: number } = {},
): { duration: number; frames: { offset: number; progress: number }[] } {
  const remaining = springRemaining(physics);
  let end = frame;
  while (Math.abs(remaining(end)) > rest && end < 10_000) end += frame;
  const frames = [];
  for (let t = 0; t <= end + 0.001; t += frame) {
    frames.push({ offset: Math.min(t / end, 1), progress: t >= end ? 1 : 1 - remaining(t) });
  }
  return { duration: end, frames };
}
