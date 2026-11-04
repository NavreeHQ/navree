/** Number formatting shared by server and client components. */

const int = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

export function fmtInt(n: number): string {
  return int.format(Math.round(n));
}

/** A count in Indian digit grouping, as the site writes its counts: 2,00,000 and 54,99,392. */
export function indian(n: number): string {
  return n.toLocaleString("en-IN");
}

/** 214,257 → "214k", 1,233,097 → "1.23M", 6,126 → "6.1k", 927 → "927". */
export function fmtCompact(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 1 : 2)}M`;
  if (n >= 100_000) return `${Math.round(n / 1000)}k`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  if (n >= 1000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  return fmtInt(n);
}

/** Latency: one decimal below 10 ms, whole numbers above. */
export function fmtMs(n: number): string {
  return n < 10 ? n.toFixed(1) : fmtInt(n);
}

/** Memory: one decimal below 10 MB, whole numbers above, GB from 1,000 MB. */
export function fmtMb(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)} GB`;
  return `${n < 10 ? n.toFixed(1) : fmtInt(n)} MB`;
}

export function fmtPct(n: number, digits = 1): string {
  const sign = n > 0 ? "+" : n < 0 ? "−" : "±";
  return `${sign}${Math.abs(n).toFixed(digits)}%`;
}

/** 1 → "1st", 2 → "2nd", 13 → "13th". */
export function ordinal(n: number): string {
  const rem100 = n % 100;
  if (rem100 >= 11 && rem100 <= 13) return `${n}th`;
  return `${n}${({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[n % 10] ?? "th"}`;
}

/** "3.3×" for ratios, "1.4×" with one decimal. */
export function fmtTimes(n: number): string {
  return `${n >= 10 ? Math.round(n) : n.toFixed(1)}×`;
}
