/**
 * Excerpts of real code, cut when the site is built from the file (or the
 * example) that holds it, so a page never shows code that isn't in the
 * repository. Server components and scripts only.
 */
import fs from "node:fs";
import path from "node:path";

/** The repository's root, from `apps/website`. */
const REPO = path.resolve(process.cwd(), "..", "..");

/**
 * One run of lines: from the first line that starts with `from` (leading
 * spaces aside) to the first line from there that starts with `to`, both
 * kept. Without `to`, the one line.
 */
export type ExcerptPart = { from: string; to?: string };

export type Excerpt = {
  /** Where the code lives, from the repository's root. */
  file: string;
  /** The runs of lines to keep, in the file's order. */
  parts: ExcerptPart[];
  /** End with a `// …` line: the excerpt stops before its block does. */
  more?: boolean;
};

/** The longest excerpt a page shows. */
export const MAX_EXCERPT_LINES = 35;

/**
 * Cuts `parts` out of `source` and joins them with a `// …` line at the
 * indentation of the part that follows, then removes the indentation every
 * line shares. A part that isn't found, or a result longer than `max`
 * lines, fails the build with `label` in the message.
 */
export function cut(source: string, parts: ExcerptPart[], label: string, options: { more?: boolean; max?: number } = {}): string {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const starts = (line: string, anchor: string) => line.trimStart().startsWith(anchor);
  const out: string[] = [];
  let at = 0;
  for (const part of parts) {
    const first = lines.findIndex((line, i) => i >= at && starts(line, part.from));
    if (first < 0) throw new Error(`excerpt: no line starting with "${part.from}" in ${label}`);
    const last = part.to === undefined ? first : lines.findIndex((line, i) => i >= first && starts(line, part.to as string));
    if (last < 0) throw new Error(`excerpt: no line starting with "${part.to}" after "${part.from}" in ${label}`);
    if (out.length) out.push(`${indentOf(lines[first])}// …`);
    out.push(...lines.slice(first, last + 1));
    at = last + 1;
  }
  if (options.more) {
    // Inside the block the last line opens, or beside the last line.
    const last = out.at(-1) ?? "";
    out.push(`${indentOf(last)}${/[{([]\s*$/.test(last) ? "    " : ""}// …`);
  }
  const shared = Math.min(...out.filter((line) => line.trim()).map((line) => indentOf(line).length));
  const text = out.map((line) => line.slice(shared)).join("\n");
  const max = options.max ?? MAX_EXCERPT_LINES;
  if (out.length > max) throw new Error(`excerpt: ${label} is ${out.length} lines, more than ${max}`);
  return text;
}

/** An excerpt of a file in the repository. */
export function excerpt({ file, parts, more }: Excerpt, max?: number): string {
  const source = fs.readFileSync(path.join(REPO, file), "utf8");
  return cut(source, parts, file, { more, max });
}

function indentOf(line: string): string {
  return line.match(/^\s*/)?.[0] ?? "";
}
