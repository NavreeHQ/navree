/**
 * List queries: the filters, sort, search and paging of a Navree list
 * endpoint, in the query-string format its `ListQuery` extractor reads:
 *
 * ```text
 * filter[status]=active                  equal
 * filter[created_at][gte]=2026-10-01     an operator
 * filter[stage][in]=lead,won             one of several values
 * sort=-created_at,name                  descending with a leading "-"
 * search=acme
 * page=2&per_page=25                     offset pages, or
 * cursor=eyJpZCI6NDJ9&limit=50           cursor pages
 * ```
 */

/** The operators a filter can use besides plain equality. */
export type FilterOperator =
  | "eq"
  | "ne"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "in"
  | "not_in"
  | "contains"
  | "starts_with"
  | "null";

/** One value: dates are sent as ISO 8601, booleans and numbers as text. */
export type FilterScalar = string | number | boolean | Date;
/** A value, or a list (sent as `in`). */
export type FilterValue = FilterScalar | readonly FilterScalar[];
/** A field's filter: a value (equality), or operators and their values. */
export type FilterCondition = FilterValue | Partial<Record<FilterOperator, FilterValue>>;

/** A list query, as data. Every member is optional. */
export interface ListQuery {
  filter?: Record<string, FilterCondition>;
  /** Fields to sort by, in order; `-` in front sorts descending. */
  sort?: string[];
  search?: string;
  page?: number;
  per_page?: number;
  cursor?: string;
  limit?: number;
}

const OPERATORS = new Set<string>(["eq", "ne", "gt", "gte", "lt", "lte", "in", "not_in", "contains", "starts_with", "null"]);
const LIST_OPERATORS = new Set<string>(["in", "not_in"]);

function isScalar(value: unknown): value is FilterScalar {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean" || value instanceof Date;
}

function isOperatorMap(value: unknown): value is Partial<Record<FilterOperator, FilterValue>> {
  return typeof value === "object" && value !== null && !Array.isArray(value) && !(value instanceof Date);
}

function text(value: FilterScalar): string {
  return value instanceof Date ? value.toISOString() : String(value);
}

function listText(value: FilterValue): string {
  return Array.isArray(value) ? value.map((v) => text(v as FilterScalar)).join(",") : text(value as FilterScalar);
}

function present(value: unknown): boolean {
  if (value === undefined || value === null) return false;
  if (typeof value === "string") return value.trim() !== "";
  if (Array.isArray(value)) return value.length > 0;
  return true;
}

function appendFilter(params: URLSearchParams, field: string, condition: FilterCondition | undefined): void {
  if (!present(condition)) return;
  if (Array.isArray(condition)) {
    params.append(`filter[${field}][in]`, listText(condition));
  } else if (isOperatorMap(condition)) {
    for (const [op, value] of Object.entries(condition)) {
      if (!present(value)) continue;
      params.append(`filter[${field}][${op}]`, listText(value as FilterValue));
    }
  } else if (isScalar(condition)) {
    params.append(`filter[${field}]`, text(condition));
  }
}

/** Writes a list query as URL parameters (filters, sort, search, page, per_page, cursor, limit). */
export function serializeListQuery(query: ListQuery): URLSearchParams {
  const params = new URLSearchParams();
  for (const [field, condition] of Object.entries(query.filter ?? {})) appendFilter(params, field, condition);
  if (query.sort && query.sort.length > 0) params.set("sort", query.sort.join(","));
  if (query.search !== undefined && query.search.trim() !== "") params.set("search", query.search);
  if (query.page !== undefined) params.set("page", String(query.page));
  if (query.per_page !== undefined) params.set("per_page", String(query.per_page));
  if (query.cursor) params.set("cursor", query.cursor);
  if (query.limit !== undefined) params.set("limit", String(query.limit));
  return params;
}

type ParamSource = URLSearchParams | string | Record<string, string | string[] | undefined>;

function entries(input: ParamSource): [string, string][] {
  if (typeof input === "string") return [...new URLSearchParams(input.startsWith("?") ? input.slice(1) : input)];
  if (input instanceof URLSearchParams) return [...input];
  const out: [string, string][] = [];
  for (const [key, value] of Object.entries(input)) {
    if (value === undefined) continue;
    const first = Array.isArray(value) ? value[0] : value;
    if (first !== undefined) out.push([key, first]);
  }
  return out;
}

function positiveInt(value: string): number | undefined {
  if (!/^\d+$/.test(value)) return undefined;
  const n = Number.parseInt(value, 10);
  return n > 0 ? n : undefined;
}

const FILTER_KEY = /^filter\[([^\]]+)\](?:\[([a-z_]+)\])?$/;

/**
 * Reads a list query from URL parameters: a `URLSearchParams`, a query
 * string (with or without `?`), or a Next.js `searchParams` object.
 * Parameters that aren't part of a list query are ignored, and so are
 * page numbers that aren't positive integers.
 */
export function parseListQuery(input: ParamSource): ListQuery {
  const query: ListQuery = {};
  for (const [key, value] of entries(input)) {
    const match = FILTER_KEY.exec(key);
    if (match) {
      const field = match[1] as string;
      const op = match[2];
      if (value === "") continue;
      query.filter ??= {};
      if (op === undefined) {
        query.filter[field] = value;
      } else if (OPERATORS.has(op)) {
        const current = query.filter[field];
        const ops = isOperatorMap(current) ? current : {};
        const parsed: FilterValue = LIST_OPERATORS.has(op)
          ? value.split(",").filter((v) => v !== "")
          : op === "null"
            ? value === "true" || value === "1"
            : value;
        query.filter[field] = { ...ops, [op]: parsed };
      }
      continue;
    }
    switch (key) {
      case "sort": {
        const fields = value.split(",").map((f) => f.trim()).filter(Boolean);
        if (fields.length > 0) query.sort = fields;
        break;
      }
      case "search":
        if (value.trim() !== "") query.search = value;
        break;
      case "page":
      case "per_page":
      case "limit": {
        const n = positiveInt(value);
        if (n !== undefined) query[key] = n;
        break;
      }
      case "cursor":
        if (value !== "") query.cursor = value;
        break;
    }
  }
  return query;
}

function withFilter(query: ListQuery, field: string, condition: FilterCondition | undefined): ListQuery {
  const filter = { ...query.filter };
  if (condition === undefined) delete filter[field];
  else filter[field] = condition;
  const { filter: _old, ...rest } = query;
  return Object.keys(filter).length > 0 ? { ...rest, filter } : rest;
}

/** An immutable builder for a {@link ListQuery}: every call returns a new builder. */
export class ListQueryBuilder {
  constructor(private readonly query: ListQuery = {}) {}

  /** `filter[field]=value`. */
  where(field: string, value: FilterValue): ListQueryBuilder;
  /** `filter[field][op]=value`; operators on one field add up. */
  where(field: string, op: FilterOperator, value: FilterValue): ListQueryBuilder;
  where(field: string, opOrValue: FilterValue | FilterOperator, value?: FilterValue): ListQueryBuilder {
    if (value === undefined) {
      const plain = opOrValue as FilterValue;
      return new ListQueryBuilder(withFilter(this.query, field, Array.isArray(plain) ? { in: plain } : plain));
    }
    const current = this.query.filter?.[field];
    const ops = isOperatorMap(current) ? current : {};
    return new ListQueryBuilder(withFilter(this.query, field, { ...ops, [opOrValue as FilterOperator]: value }));
  }

  /** `filter[field][in]=a,b`. */
  whereIn(field: string, values: readonly FilterScalar[]): ListQueryBuilder {
    return this.where(field, "in", values);
  }

  /** Drops the filter on `field`. */
  without(field: string): ListQueryBuilder {
    return new ListQueryBuilder(withFilter(this.query, field, undefined));
  }

  /** Sort fields, `-` in front for descending: `sort("-created_at", "name")`. */
  sort(...fields: string[]): ListQueryBuilder {
    return new ListQueryBuilder({ ...this.query, sort: fields });
  }

  search(text: string): ListQueryBuilder {
    return new ListQueryBuilder({ ...this.query, search: text });
  }

  page(page: number): ListQueryBuilder {
    const { cursor: _c, limit: _l, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, page });
  }

  perPage(perPage: number): ListQueryBuilder {
    const { cursor: _c, limit: _l, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, per_page: perPage });
  }

  /** Cursor paging: the `next_cursor` of the previous page. Drops `page` and `per_page`. */
  cursor(cursor: string): ListQueryBuilder {
    const { page: _p, per_page: _pp, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, cursor });
  }

  limit(limit: number): ListQueryBuilder {
    const { page: _p, per_page: _pp, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, limit });
  }

  /** The query as data, for the client's `params.query` and query keys. */
  toQuery(): ListQuery {
    return structuredClone(this.query);
  }

  toSearchParams(): URLSearchParams {
    return serializeListQuery(this.query);
  }

  toString(): string {
    return this.toSearchParams().toString();
  }
}

/** Starts a list query, optionally from an existing one. */
export function listQuery(initial: ListQuery = {}): ListQueryBuilder {
  return new ListQueryBuilder(structuredClone(initial));
}

function appendParam(params: URLSearchParams, key: string, value: unknown): void {
  if (value === undefined || value === null) return;
  if (Array.isArray(value)) {
    for (const item of value) appendParam(params, key, item);
  } else if (value instanceof Date) {
    params.append(key, value.toISOString());
  } else if (typeof value === "object") {
    for (const [k, v] of Object.entries(value)) appendParam(params, `${key}[${k}]`, v);
  } else {
    params.append(key, String(value));
  }
}

/**
 * A `querySerializer` for the typed client: `filter` and `sort` in list-query
 * format, other objects in brackets (`a[b]=c`), other lists repeated
 * (`ids=a&ids=b`).
 */
export function listQuerySerializer(query: object): string {
  const record = query as Record<string, unknown>;
  const params = serializeListQuery({
    filter: record.filter as ListQuery["filter"],
    sort: typeof record.sort === "string" ? [record.sort] : (record.sort as string[] | undefined),
  });
  for (const [key, value] of Object.entries(record)) {
    if (key === "filter" || key === "sort") continue;
    appendParam(params, key, value);
  }
  return params.toString();
}

// ---- Sorting ---------------------------------------------------------------------

/** The direction `field` is sorted in, if it is. */
export function sortDirection(sort: readonly string[] | undefined, field: string): "asc" | "desc" | undefined {
  for (const entry of sort ?? []) {
    if (entry === field) return "asc";
    if (entry === `-${field}`) return "desc";
  }
  return undefined;
}

/**
 * The sort after clicking `field`'s heading: ascending, then descending,
 * then off. Other fields are dropped unless `multi` is set.
 */
export function toggleSort(
  sort: readonly string[] | undefined,
  field: string,
  { multi = false }: { multi?: boolean } = {},
): string[] {
  const current = sortDirection(sort, field);
  const next = current === undefined ? field : current === "asc" ? `-${field}` : undefined;
  if (!multi) return next ? [next] : [];
  const out: string[] = [];
  let placed = false;
  for (const entry of sort ?? []) {
    if (entry === field || entry === `-${field}`) {
      if (next) out.push(next);
      placed = true;
    } else {
      out.push(entry);
    }
  }
  if (!placed && next) out.push(next);
  return out;
}

/** TanStack Table's sorting state for a list-query sort. */
export function toSorting(sort: readonly string[] | undefined): { id: string; desc: boolean }[] {
  return (sort ?? []).map((entry) =>
    entry.startsWith("-") ? { id: entry.slice(1), desc: true } : { id: entry, desc: false },
  );
}

/** The list-query sort for TanStack Table's sorting state. */
export function fromSorting(sorting: readonly { id: string; desc: boolean }[]): string[] {
  return sorting.map(({ id, desc }) => (desc ? `-${id}` : id));
}

// ---- Updates ---------------------------------------------------------------------

/** `query` with `field`'s filter set (or removed with `undefined`), back on the first page. */
export function setFilter(query: ListQuery, field: string, condition: FilterCondition | undefined): ListQuery {
  const blank = condition === undefined || !present(condition);
  const { page: _page, cursor: _cursor, ...rest } = withFilter(query, field, blank ? undefined : condition);
  return rest;
}

/** `query` on `page` (page 1 is left out of the URL). */
export function withPage(query: ListQuery, page: number): ListQuery {
  const { page: _page, ...rest } = query;
  return page > 1 ? { ...rest, page } : rest;
}
