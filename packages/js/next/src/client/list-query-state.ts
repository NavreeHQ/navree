"use client";

/**
 * `useListQuery()`: a list's filters, sort, search and page in the URL, so a
 * list survives a reload, the back button and a shared link.
 */
import {
  parseListQuery,
  serializeListQuery,
  setFilter as withFilter,
  toggleSort as nextSort,
  withPage,
  type FilterCondition,
  type ListQuery,
} from "@navree/client";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useCallback, useMemo } from "react";

/** True for the URL parameters a list query owns. */
export function isListParam(key: string): boolean {
  return key.startsWith("filter[") || ["sort", "search", "page", "per_page", "cursor", "limit"].includes(key);
}

/** `params` with its list-query parameters replaced by `query`'s (other parameters stay). */
export function mergeListParams(params: URLSearchParams, query: ListQuery, defaults: ListQuery = {}): URLSearchParams {
  const merged = new URLSearchParams();
  params.forEach((value, key) => {
    if (!isListParam(key)) merged.append(key, value);
  });
  const trimmed: ListQuery = { ...query };
  for (const key of Object.keys(defaults) as (keyof ListQuery)[]) {
    if (JSON.stringify(trimmed[key]) === JSON.stringify(defaults[key])) delete trimmed[key];
  }
  serializeListQuery(trimmed).forEach((value, key) => merged.append(key, value));
  return merged;
}

export interface ListQueryState {
  /** The list query in the URL, over `defaults`. */
  query: ListQuery;
  /** Replaces the list query (other URL parameters stay). */
  setQuery: (next: ListQuery | ((current: ListQuery) => ListQuery)) => void;
  /** Sets one filter (or removes it with `undefined`) and goes back to page 1. */
  setFilter: (field: string, condition: FilterCondition | undefined) => void;
  /** Sets the search text and goes back to page 1. */
  setSearch: (search: string) => void;
  /** Ascending, descending, off; other fields dropped unless `multi`. */
  toggleSort: (field: string, options?: { multi?: boolean }) => void;
  setPage: (page: number) => void;
}

/**
 * The list query of the current URL. Updates use `router.replace` (no
 * history entry per keystroke) and keep the page's other parameters.
 */
export function useListQuery(defaults: ListQuery = {}): ListQueryState {
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();
  const defaultsKey = JSON.stringify(defaults);
  const search = params?.toString() ?? "";

  const query = useMemo<ListQuery>(
    () => ({ ...(JSON.parse(defaultsKey) as ListQuery), ...parseListQuery(search) }),
    [defaultsKey, search],
  );

  const setQuery = useCallback(
    (next: ListQuery | ((current: ListQuery) => ListQuery)) => {
      const value = typeof next === "function" ? next(query) : next;
      const merged = mergeListParams(new URLSearchParams(search), value, JSON.parse(defaultsKey) as ListQuery);
      const qs = merged.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [query, search, defaultsKey, pathname, router],
  );

  return useMemo(
    () => ({
      query,
      setQuery,
      setFilter: (field, condition) => setQuery((q) => withFilter(q, field, condition)),
      setSearch: (text) =>
        setQuery((q) => {
          const { page: _page, cursor: _cursor, search: _search, ...rest } = q;
          return text.trim() ? { ...rest, search: text } : rest;
        }),
      toggleSort: (field, options) =>
        setQuery((q) => {
          const { page: _page, cursor: _cursor, ...rest } = q;
          const sort = nextSort(q.sort, field, options);
          if (sort.length > 0) return { ...rest, sort };
          const { sort: _sort, ...unsorted } = rest;
          return unsorted;
        }),
      setPage: (page) => setQuery((q) => withPage(q, page)),
    }),
    [query, setQuery],
  );
}
