/**
 * `useListQuery()`: a list's filters, sort, search and page in the route's
 * query string (vue-router), so a list survives a reload and a shared link.
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
import { computed, type ComputedRef } from "vue";
import { useRoute, useRouter, type LocationQuery, type LocationQueryRaw } from "vue-router";

/** True for the query parameters a list query owns. */
export function isListParam(key: string): boolean {
  return key.startsWith("filter[") || ["sort", "search", "page", "per_page", "cursor", "limit"].includes(key);
}

function flatten(query: LocationQuery): Record<string, string | string[] | undefined> {
  const out: Record<string, string | string[] | undefined> = {};
  for (const [key, value] of Object.entries(query)) {
    if (value === null || value === undefined) continue;
    out[key] = Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : value;
  }
  return out;
}

/** The route query with its list parameters replaced by `query`'s (others stay; values equal to `defaults` are left out). */
export function mergeListQuery(current: LocationQuery, query: ListQuery, defaults: ListQuery = {}): LocationQueryRaw {
  const merged: LocationQueryRaw = {};
  for (const [key, value] of Object.entries(current)) {
    if (!isListParam(key)) merged[key] = value;
  }
  const trimmed: ListQuery = { ...query };
  for (const key of Object.keys(defaults) as (keyof ListQuery)[]) {
    if (JSON.stringify(trimmed[key]) === JSON.stringify(defaults[key])) delete trimmed[key];
  }
  serializeListQuery(trimmed).forEach((value, key) => {
    const existing = merged[key];
    merged[key] = existing === undefined ? value : ([] as unknown[]).concat(existing, value) as string[];
  });
  return merged;
}

export interface ListQueryState {
  query: ComputedRef<ListQuery>;
  setQuery(next: ListQuery | ((current: ListQuery) => ListQuery)): Promise<void>;
  setFilter(field: string, condition: FilterCondition | undefined): Promise<void>;
  setSearch(search: string): Promise<void>;
  toggleSort(field: string, options?: { multi?: boolean }): Promise<void>;
  setPage(page: number): Promise<void>;
}

/** The list query of the current route. Updates use `router.replace` and keep the route's other parameters. */
export function useListQuery(defaults: ListQuery = {}): ListQueryState {
  const route = useRoute();
  const router = useRouter();
  const query = computed<ListQuery>(() => ({ ...structuredClone(defaults), ...parseListQuery(flatten(route.query)) }));

  const setQuery = async (next: ListQuery | ((current: ListQuery) => ListQuery)) => {
    const value = typeof next === "function" ? next(query.value) : next;
    await router.replace({ query: mergeListQuery(route.query, value, defaults) });
  };

  return {
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
        const { page: _page, cursor: _cursor, sort: _sort, ...rest } = q;
        const sort = nextSort(q.sort, field, options);
        return sort.length > 0 ? { ...rest, sort } : rest;
      }),
    setPage: (page) => setQuery((q) => withPage(q, page)),
  };
}
