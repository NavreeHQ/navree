/**
 * What the flags weigh, in words, from the numbers `cargo run -p ui-docs` measures in the
 * committed artwork (`toolkit.json`, `flag_weights`). One function for the page and for the
 * Markdown copies, so the two agree and neither can drift from the data.
 *
 * Node runs this with the types stripped: no imports.
 */
export type FlagWeight = { code: string; name: string; bytes: number };
export type FlagWeights = {
  flags: number;
  median: number;
  over_2_kb: number;
  over_10_kb: number;
  over_50_kb: number;
  /** The heaviest first. */
  heaviest: FlagWeight[];
  /** A few flags people use, for scale. */
  plain: FlagWeight[];
};

/** `661 bytes`, `4.8 KB`, `175 KB`. */
export function weight(bytes: number): string {
  if (bytes < 1024) return `${bytes.toLocaleString("en-US")} bytes`;
  const kb = bytes / 1024;
  return `${kb < 10 ? kb.toFixed(1) : Math.round(kb).toLocaleString("en-US")} KB`;
}

/** The paragraph under "What they weigh". */
export function flagWeightsText(w: FlagWeights): string {
  const plain = w.plain.map((f) => `${f.name} is ${weight(f.bytes)}`).join(", ");
  const heaviest = w.heaviest.map((f) => `${f.name} ${weight(f.bytes)}`).join(", ");
  return (
    "The artwork is inline, so a flag costs its bytes on every page it appears on. " +
    `The median flag is ${weight(w.median)} (${plain}). ` +
    `${w.over_2_kb} of the ${w.flags} are over 2 KB, ${w.over_10_kb} over 10 KB and ${w.over_50_kb} over 50 KB. ` +
    `The heaviest carry a coat of arms: ${heaviest}. ` +
    `A table of two hundred rows of ${w.heaviest[0].name} would carry it two hundred times. ` +
    "Where one country repeats down a column, show its name in text and the flag only where it helps; " +
    "browsers compress the repeats well on the wire, but the page still holds every copy."
  );
}
