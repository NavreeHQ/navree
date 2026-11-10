/** `next` when it is a path on this site (never an open redirect), else `/`. */
export function safeNext(next: string | null | undefined): string {
  return next && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/\\") ? next : "/";
}

/** `?next=<path>` for a link that comes back to `next`, or nothing for `/`. */
export function nextQuery(next: string): string {
  return next === "/" ? "" : `?next=${encodeURIComponent(next)}`;
}
