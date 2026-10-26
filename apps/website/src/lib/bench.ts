import data from "@/generated/performance.json";

import type { PerfData, TestKey, TestResult } from "./bench-types";

/**
 * Navree's own performance measurements. Every number on the site comes from
 * here, and this comes from `bench/results/navree.json` (see scripts/prebuild.ts).
 */
export const perf = data as PerfData;

export { fmtCompact, fmtInt, fmtMb, fmtMs, fmtPct } from "./format";

export function testInfo(key: TestKey) {
  const test = perf.tests.find((t) => t.key === key);
  if (!test) throw new Error(`unknown test ${key}`);
  return test;
}

export function result(key: TestKey): TestResult {
  return perf.results[key];
}

/** Every test with its result, in the harness's order. */
export function testsWithResults() {
  return perf.tests.map((test) => ({ ...test, result: perf.results[test.key] }));
}

/** Errors across every measured run of every test. */
export function totalErrors(): number {
  return Object.values(perf.results).reduce((sum, r) => sum + r.runs.reduce((s, run) => s + run.errors, 0), 0);
}

/** How many measured runs there were in all (tests × runs). */
export function measuredRuns(): number {
  return Object.values(perf.results).reduce((sum, r) => sum + r.runs.length, 0);
}

/** The worst p99/p50 ratio across the tests: how far the tail strays from the median. */
export function worstTailRatio(): number {
  return Math.max(...Object.values(perf.results).map((r) => r.latencyMs.p99 / r.latencyMs.p50));
}

/** The largest change between the measured run and the repeat, in percent. */
export function maxRepeatDelta(): number | null {
  if (!perf.repeat) return null;
  return Math.max(...perf.repeat.tests.map((t) => Math.abs(t.deltaPct)));
}

/** The largest spread between a test's three runs, as a percentage of its median. */
export function maxRunSpread(): number {
  return Math.max(
    ...Object.values(perf.results).map((r) => {
      const values = r.runs.map((run) => run.rps);
      return ((Math.max(...values) - Math.min(...values)) / r.rps) * 100;
    }),
  );
}

/** Memory figures keep one decimal below 100 MB: 37.5, 4.2. */
export function mb(n: number): number {
  return n < 100 ? Math.round(n * 10) / 10 : Math.round(n);
}

/** CPU percent of one core as CPUs, one decimal: 771.98 → "7.7". */
export function cpus(pct: number): string {
  return (pct / 100).toFixed(1);
}

/** "October 26, 2026". */
export function measuredOn(): string {
  const fmt = new Intl.DateTimeFormat("en-US", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
  return fmt.format(new Date(perf.startedAt));
}

/** A "Name x.y.z" version out of a README stack cell, e.g. `Rust 1.99.1`. */
export function stackVersion(label: string, pattern: RegExp): string | null {
  const row = perf.stack.find((r) => r.label === label);
  return row ? (pattern.exec(row.html)?.[0] ?? null) : null;
}
