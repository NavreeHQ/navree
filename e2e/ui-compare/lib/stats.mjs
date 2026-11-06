// Medians, ranges and the words used for numbers in the report.

/** The middle value of a list of numbers (the mean of the two middle ones for an even count). */
export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  if (sorted.length === 0) return null;
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

/** The median, the least and the greatest of the numbers in `values`, or null when there are none. */
export function summarize(values) {
  const numbers = values.filter((value) => typeof value === "number" && Number.isFinite(value));
  if (numbers.length === 0) return null;
  return { n: numbers.length, median: median(numbers), min: Math.min(...numbers), max: Math.max(...numbers), values: numbers };
}

/** 1536 as "1.5 KB"; under 1 KB as "512 B". */
export function bytesText(bytes) {
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const kilobytes = bytes / 1024;
  if (kilobytes < 1024) return `${kilobytes < 100 ? kilobytes.toFixed(1) : Math.round(kilobytes)} KB`;
  return `${(kilobytes / 1024).toFixed(2)} MB`;
}

/** 83.4 as "83 ms"; under 10 ms with one decimal; from 10 s as seconds. */
export function msText(ms) {
  if (ms < 10) return `${ms.toFixed(1)} ms`;
  if (ms < 10000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(1)} s`;
}

export const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
