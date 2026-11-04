/**
 * The code typed on the live picture's screen, as a schedule: when each
 * character appears within one round of typing, and how long a round is.
 * A round types every line, holds the finished code, clears the screen and
 * rests, then the next round starts, so the screen at time `t` depends on
 * `t` alone.
 */
import { BEATS, CARET_HZ, TYPING } from "./timeline.ts";

export type TypedChar = { ch: string; line: number; col: number; at: number };
export type Schedule = { chars: TypedChar[]; done: number; period: number };

const indentOf = (line: string) => line.length - line.trimStart().length;

export function schedule(lines: readonly string[]): Schedule {
  const chars: TypedChar[] = [];
  let at = 0.4;
  lines.forEach((line, i) => {
    if (!line.trim()) {
      at += TYPING.blank;
      return;
    }
    for (let col = indentOf(line); col < line.length; col++) {
      if (line[col] !== " ") chars.push({ ch: line[col], line: i, col, at });
      at += TYPING.char;
    }
    at += TYPING.line;
  });
  return { chars, done: at, period: at + TYPING.hold + TYPING.clear + TYPING.rest };
}

/** Seconds into the current round of typing at time `t` (negative before typing starts). */
export function roundTime(t: number, s: Schedule): number {
  const u = t - BEATS.typeFrom;
  return u < 0 ? u : u % s.period;
}

/**
 * Where the caret is at time `t` (line and column, in characters) and
 * whether it shows: steady while typing, blinking while it waits.
 */
export function caretAt(t: number, s: Schedule, lines: readonly string[]): { line: number; col: number; on: boolean } {
  const u = roundTime(t, s);
  const blink = Math.floor(t * 2 * CARET_HZ) % 2 === 0;
  if (u >= s.done + TYPING.hold) return { line: 0, col: 0, on: false };
  // The last character typed so far.
  let lo = -1;
  let hi = s.chars.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (s.chars[mid].at <= u) lo = mid;
    else hi = mid - 1;
  }
  if (lo < 0) return { line: 0, col: indentOf(lines[0]), on: blink };
  const last = s.chars[lo];
  const next = s.chars[lo + 1];
  if (next && next.line !== last.line && u - last.at > TYPING.char * 1.5) {
    // Between lines: the caret waits at the start of the next one.
    return { line: next.line, col: indentOf(lines[next.line]), on: blink };
  }
  const typing = next !== undefined && next.at - u < TYPING.char * 2.5;
  return { line: last.line, col: last.col + 1, on: typing || blink };
}
