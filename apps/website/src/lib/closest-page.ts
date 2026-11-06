/**
 * The page of the site nearest to an address that is not there, for the 404
 * page ("Did you mean ...?"). Plain functions with relative imports only, so
 * `scripts/check-404.ts` runs them as they are.
 */

/** One page the site has: `scripts/prebuild.ts` writes the list to `/data/pages.json`. */
export type PageEntry = { href: string; title: string; section: string };

/** Addresses the site used to have, and the pages that took their place. */
const MOVED: Record<string, string> = {
  "/benchmarks/": "/performance/",
};

/** The score a page needs to be suggested: a slip of a letter or two, a missing section, not a page that merely looks a little alike. */
const NEAR = 0.72;

/** Edit distance between two short strings. */
function distance(a: string, b: string): number {
  let previous = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const row = [i];
    for (let j = 1; j <= b.length; j++) {
      row[j] = Math.min(previous[j] + 1, row[j - 1] + 1, previous[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    }
    previous = row;
  }
  return previous[b.length];
}

/** 1 for equal strings, 0 for strings with nothing in common. */
function similarity(a: string, b: string): number {
  if (!a || !b) return 0;
  return a === b ? 1 : 1 - distance(a, b) / Math.max(a.length, b.length);
}

/** The words of an address: its segments, split further at dashes and underscores. */
const words = (text: string) => text.split(/[^a-z0-9]+/).filter(Boolean);

/**
 * The page nearest to `pathname`, or null when nothing is near enough to
 * suggest. A page scores by how close its whole address is, how close its last
 * segment is to the one asked for (so `/docs/validation/` finds
 * `/docs/basics/validation/`), and how many of the words asked for its title
 * holds. An address the site used to have goes to its new page.
 */
export function closestPage(pathname: string, pages: readonly PageEntry[]): PageEntry | null {
  const asked = pathname.split(/[?#]/)[0].toLowerCase().replace(/\/+$/, "");
  if (!asked || pages.length === 0) return null;

  const moved = MOVED[`${asked}/`];
  if (moved) return pages.find((page) => page.href === moved) ?? null;

  const segments = asked.split("/").filter(Boolean);
  const last = segments.at(-1) ?? "";
  const wanted = words(asked);
  let best: { page: PageEntry; score: number } | null = null;
  for (const page of pages) {
    const own = page.href.split("/").filter(Boolean);
    const whole = similarity(segments.join("/"), own.join("/"));
    const tail = similarity(last, own.at(-1) ?? "");
    const title = words(page.title.toLowerCase());
    const shared = wanted.length ? wanted.filter((w) => title.includes(w)).length / wanted.length : 0;
    // The last segment says most about what was meant; the whole address breaks ties between pages that share it.
    const score = Math.max(whole, tail * 0.9 + whole * 0.1, shared * 0.8);
    if (!best || score > best.score) best = { page, score };
  }
  return best && best.score >= NEAR ? best.page : null;
}
