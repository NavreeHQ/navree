import data from "@/generated/benchmarks.json";

import type { BenchData, Framework, TestKey } from "./bench-types";

/** Every benchmark number on the site comes from here (see scripts/prebuild.ts). */
export const bench = data as BenchData;

export function highlighted(): Framework {
  const found = bench.frameworks.find((f) => f.name === bench.highlight);
  if (!found) throw new Error(`no results for ${bench.highlight}`);
  return found;
}

export function testInfo(key: TestKey) {
  const test = bench.tests.find((t) => t.key === key);
  if (!test) throw new Error(`unknown test ${key}`);
  return test;
}

export { fmtCompact, fmtInt, fmtMb, fmtMs, fmtPct, fmtTimes, ordinal } from "./format";

// ------------------------------------------------------------ comparisons

export function rps(f: Framework, key: TestKey): number {
  return f.results[key]?.rps ?? 0;
}

/** Frameworks with a result for `key`, fastest first. */
export function rankedBy(key: TestKey, category?: Framework["category"]): Framework[] {
  return bench.frameworks
    .filter((f) => f.results[key] && (!category || f.category === category))
    .sort((a, b) => rps(b, key) - rps(a, key));
}

/** The highlighted framework's place in `key`, among all or one category. */
export function rankIn(key: TestKey, category?: Framework["category"]): { rank: number; of: number } {
  const list = rankedBy(key, category);
  return { rank: list.findIndex((f) => f.name === bench.highlight) + 1, of: list.length };
}

/** Rank by a "lower is better" metric. */
export function rankLowest(metric: (f: Framework) => number, category?: Framework["category"]): { rank: number; of: number } {
  const list = bench.frameworks.filter((f) => !category || f.category === category).sort((a, b) => metric(a) - metric(b));
  return { rank: list.findIndex((f) => f.name === bench.highlight) + 1, of: list.length };
}

export function byName(name: string): Framework | undefined {
  return bench.frameworks.find((f) => f.name === name);
}

/** The date range the results were measured in, e.g. "26 October 2026". */
export function measuredOn(): string {
  const dates = bench.frameworks.map((f) => f.startedAt).filter(Boolean).sort();
  if (!dates.length) return "";
  const fmt = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
  const first = fmt.format(new Date(dates[0]));
  const last = fmt.format(new Date(dates[dates.length - 1]));
  return first === last ? first : `${first} to ${last}`;
}
