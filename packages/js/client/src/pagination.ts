/**
 * Navree's page shapes. Offset pages (`?page=&per_page=`):
 *
 * ```json
 * {"data": [...], "meta": {"current_page": 2, "per_page": 25, "total": 60, "last_page": 3}}
 * ```
 *
 * and cursor pages (`?cursor=&limit=`):
 *
 * ```json
 * {"data": [...], "next_cursor": "eyJpZCI6NDJ9"}
 * ```
 */
import type { ListQuery } from "./list-query";

/** A facet value and how many rows have it. */
export interface FacetCount {
  value: string;
  count: number;
  label?: string;
}

/** Facets as a list endpoint sends them in `meta.facets`: per field, a map of counts or a list. */
export type Facets = Record<
  string,
  Record<string, number> | ReadonlyArray<{ value: string | number | boolean | null; count: number; label?: string }>
>;

/** `meta` of an offset page. */
export interface PageMeta {
  current_page: number;
  per_page: number;
  total: number;
  last_page: number;
  /** Counts per value of the faceted fields, when the endpoint declares facets. */
  facets?: Facets;
}

/** One offset page. */
export interface Page<T> {
  data: T[];
  meta: PageMeta;
}

/** One cursor page. */
export interface CursorPage<T> {
  data: T[];
  next_cursor: string | null;
}

export type AnyPage<T> = Page<T> | CursorPage<T>;

/** True for an offset page (`{data, meta}`). */
export function isPage<T = unknown>(value: unknown): value is Page<T> {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  const meta = v.meta as Record<string, unknown> | undefined;
  return Array.isArray(v.data) && typeof meta === "object" && meta !== null && typeof meta.current_page === "number";
}

/** True for a cursor page (`{data, next_cursor}`). */
export function isCursorPage<T = unknown>(value: unknown): value is CursorPage<T> {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return Array.isArray(v.data) && "next_cursor" in v;
}

/** True when another page follows `page`. */
export function hasNextPage(page: AnyPage<unknown>): boolean {
  if (isPage(page)) return page.meta.current_page < page.meta.last_page;
  return typeof page.next_cursor === "string" && page.next_cursor !== "";
}

/** The rows a page shows, counting from 1 (`26`–`50` of 60); `0`–`0` for an empty page. */
export function pageRange(meta: PageMeta): { from: number; to: number } {
  const from = (meta.current_page - 1) * meta.per_page + 1;
  if (meta.total === 0 || from > meta.total) return { from: 0, to: 0 };
  return { from, to: Math.min(meta.current_page * meta.per_page, meta.total) };
}

/** The counts of one faceted field, in the order the API sent them. */
export function facetCounts(meta: PageMeta, field: string): FacetCount[] {
  const facet = meta.facets?.[field];
  if (!facet) return [];
  if (Array.isArray(facet)) {
    return facet.map(({ value, count, label }) => ({
      value: value === null ? "" : String(value),
      count,
      ...(label !== undefined ? { label } : {}),
    }));
  }
  return Object.entries(facet as Record<string, number>).map(([value, count]) => ({ value, count }));
}

/** Fetches one page for a list query (an API call). */
export type PageFetcher<T> = (query: ListQuery) => Promise<AnyPage<T>>;

/**
 * Every page of a list, one after the other: offset pages by `page`, cursor
 * pages by `next_cursor`. Stops after `maxPages` (default: no limit).
 */
export async function* pages<T>(
  fetchPage: PageFetcher<T>,
  query: ListQuery = {},
  { maxPages = Number.POSITIVE_INFINITY }: { maxPages?: number } = {},
): AsyncGenerator<AnyPage<T>> {
  let next: ListQuery | undefined = query;
  let fetched = 0;
  while (next && fetched < maxPages) {
    const page: AnyPage<T> = await fetchPage(next);
    fetched += 1;
    yield page;
    if (!hasNextPage(page)) break;
    next = isPage(page)
      ? { ...query, page: page.meta.current_page + 1 }
      : { ...query, cursor: (page as CursorPage<T>).next_cursor as string };
  }
}

/** Every row of every page (for exports and pickers; mind large lists). */
export async function collectAll<T>(
  fetchPage: PageFetcher<T>,
  query: ListQuery = {},
  options: { maxPages?: number } = {},
): Promise<T[]> {
  const rows: T[] = [];
  for await (const page of pages(fetchPage, query, options)) rows.push(...page.data);
  return rows;
}
