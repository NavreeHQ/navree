"use client";

/**
 * `useListQuery()`: a list's filters, sort, search and page in the URL, so a
 * list survives a reload, the back button and a shared link. The URL is read
 * and written only through `@navree/client`'s list-query functions, so it
 * speaks the one grammar every Navree list does.
 */
import {
  checkListQuery,
  firstPage,
  isListParam,
  serializeListQuery,
  setFilter as withFilter,
  toggleSort as nextSort,
  withDefaults,
  withPage,
  type FilterCondition,
  type ListQuery,
  type ListQueryProblem,
} from "@navree/client";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useCallback, useMemo } from "react";

export { isListParam };

/** `params` with its list-query parameters replaced by `query`'s (other parameters stay; values equal to `defaults` are left out). */
export function mergeListQuery(params: URLSearchParams, query: ListQuery, defaults: ListQuery = {}): URLSearchParams {
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

/** The first name of {@link mergeListQuery}, kept so existing code compiles. */
export const mergeListParams = mergeListQuery;

export interface ListQueryState {
  /** The list query in the URL, over `defaults`. */
  query: ListQuery;
  /** What is wrong in the list query of the URL (an edited or old link): the rest of the query still applies. */
  problems: ListQueryProblem[];
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

  const checked = useMemo(() => checkListQuery(search), [search]);
  const query = useMemo<ListQuery>(
    () => withDefaults(checked.query, JSON.parse(defaultsKey) as ListQuery),
    [checked, defaultsKey],
  );

  const setQuery = useCallback(
    (next: ListQuery | ((current: ListQuery) => ListQuery)) => {
      const value = typeof next === "function" ? next(query) : next;
      const merged = mergeListQuery(new URLSearchParams(search), value, JSON.parse(defaultsKey) as ListQuery);
      const qs = merged.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [query, search, defaultsKey, pathname, router],
  );

  return useMemo(
    () => ({
      query,
      problems: checked.problems,
      setQuery,
      setFilter: (field, condition) => setQuery((q) => withFilter(q, field, condition)),
      setSearch: (text) =>
        setQuery((q) => {
          const { search: _search, ...rest } = firstPage(q);
          return text.trim() ? { ...rest, search: text } : rest;
        }),
      toggleSort: (field, options) =>
        setQuery((q) => {
          const { sort: _sort, ...rest } = firstPage(q);
          const sort = nextSort(q.sort, field, options);
          return sort.length > 0 ? { ...rest, sort } : rest;
        }),
      setPage: (page) => setQuery((q) => withPage(q, page)),
    }),
    [query, checked, setQuery],
  );
}
