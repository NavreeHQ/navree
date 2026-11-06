/**
 * List queries: the filters, sort, search and paging of a Navree list, in the
 * one query-string grammar every Navree list speaks. A Navree server reads it
 * with the `List<Resource>` extractor and a `ListSpec` allow-list, a Navree UI
 * table and an admin panel write the same names, and this file reads and
 * writes it in JavaScript. The Rust reference is `navree_http::list`; the two
 * parsers read one file of shared cases.
 *
 * ```text
 * filter[status]=active                  equal (same as filter[status][eq]=active)
 * filter[created_at][gte]=2026-10-01     an operator
 * filter[stage][in]=lead,won             one of several values; the key may repeat
 * filter[owner_id][null]=true            is NULL (false: is not NULL)
 * sort=-created_at,name                  descending with a leading "-"
 * search=acme
 * page=2&per_page=25                     numbered pages, or
 * cursor=eyJpZCI6NDJ9&limit=50           cursor pages (before=... is the page before)
 * ```
 *
 * Parameters that are not the list query's (`owner_id=3`, `range=30d`) are
 * ignored. Mixing numbered and cursor paging is a problem.
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
  /** Fields to sort by, in order; `-` in front sorts descending. At most four. */
  sort?: string[];
  search?: string;
  /** The page number, from 1 (numbered pages). */
  page?: number;
  /** The page size (numbered pages). */
  per_page?: number;
  /** The page a cursor points to: the `next_cursor` or `prev_cursor` of another page (cursor pages). */
  cursor?: string;
  /** The page before this position (cursor pages that go both ways). */
  before?: string;
  /** The page size (cursor pages). */
  limit?: number;
}

/** Something wrong in a query string: the parameter as written, and what is wrong with it. */
export interface ListQueryProblem {
  param: string;
  message: string;
}

/** What {@link checkListQuery} reads: the query it could keep, and what was wrong. */
export interface ListQueryCheck {
  query: ListQuery;
  problems: ListQueryProblem[];
}

type OperatorMap = Partial<Record<FilterOperator, FilterValue>>;

// ---- The limits the syntax allows (the same as the Rust reader's) ------------------------

/** The most bytes of a query string read. */
const MAX_QUERY_BYTES = 16 * 1024;
/** The most parameters read from a query string. */
const MAX_PARAMS = 1000;
/** The most filter conditions kept. */
const MAX_CONDITIONS = 64;
/** The most values one `in` or `not_in` condition keeps. */
const MAX_VALUES = 200;
/** The longest filter value or search text, in bytes. */
const MAX_TEXT = 256;
/** The longest cursor, in bytes. */
const MAX_CURSOR = 1024;
/** The most sort fields kept. */
const MAX_SORTS = 4;
/** The highest page number. */
const MAX_PAGE = 10_000_000;
/** The largest page size the syntax allows; a resource's `ListSpec` sets a lower one. */
const MAX_PAGE_SIZE = 10_000;

/** A field or sort name: a letter or `_`, then letters, digits, `_` and `.`, 64 characters at most. */
const NAME = /^[A-Za-z_][A-Za-z0-9_.]{0,63}$/;

const OPERATOR_NAMES: readonly FilterOperator[] = ["eq", "ne", "gt", "gte", "lt", "lte", "in", "not_in", "contains", "starts_with", "null"];
const OPERATORS: ReadonlySet<string> = new Set<string>(OPERATOR_NAMES);

/** The parameters, besides `filter[...]`, that a list query owns. */
const LIST_PARAMS: ReadonlySet<string> = new Set(["sort", "search", "page", "per_page", "cursor", "before", "limit"]);

const FILTER_HINT = "write a filter as filter[field]=value or filter[field][operator]=value";

function isOperator(name: string): name is FilterOperator {
  return OPERATORS.has(name);
}

/** True for the URL parameters a list query owns: `filter[...]`, `sort`, `search`, `page`, `per_page`, `cursor`, `before`, `limit`. */
export function isListParam(key: string): boolean {
  return key === "filter" || key.startsWith("filter[") || LIST_PARAMS.has(key);
}

// ---- Text helpers ----------------------------------------------------------------------

const encoder = new TextEncoder();

/** Whether `text` is longer than `max` bytes in UTF-8. */
function exceeds(text: string, max: number): boolean {
  // A UTF-16 unit is at most three bytes in UTF-8, so a short text never needs measuring.
  return text.length * 3 > max && encoder.encode(text).length > max;
}

/** `text` cut to at most `max` bytes, at a character boundary. */
function clipBytes(text: string, max: number): string {
  if (!exceeds(text, max)) return text;
  let bytes = 0;
  let end = 0;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    const size = code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
    if (bytes + size > max) break;
    bytes += size;
    end += char.length;
  }
  return text.slice(0, end);
}

/** The characters the Rust reader trims: Unicode `White_Space`. */
function isWhite(code: number): boolean {
  return (
    (code >= 0x09 && code <= 0x0d) ||
    code === 0x20 ||
    code === 0x85 ||
    code === 0xa0 ||
    code === 0x1680 ||
    (code >= 0x2000 && code <= 0x200a) ||
    code === 0x2028 ||
    code === 0x2029 ||
    code === 0x202f ||
    code === 0x205f ||
    code === 0x3000
  );
}

/** `text` without white space at either end (the same set as the Rust reader's `trim`). */
function trimWhite(text: string): string {
  let start = 0;
  let end = text.length;
  while (start < end && isWhite(text.charCodeAt(start))) start += 1;
  while (end > start && isWhite(text.charCodeAt(end - 1))) end -= 1;
  return text.slice(start, end);
}

/** Defines an own, ordinary property: a key such as `__proto__` stays a key. */
function define(target: object, key: string, value: unknown): void {
  Object.defineProperty(target, key, { value, enumerable: true, writable: true, configurable: true });
}

/** The query parameter that carries a condition: `filter[stage]` for equality, `filter[amount][gte]` for the rest. */
function paramOf(field: string, op: FilterOperator): string {
  return op === "eq" ? `filter[${field}]` : `filter[${field}][${op}]`;
}

// ---- Reading ---------------------------------------------------------------------------

/** One filter as read: a field, an operator and its values (one, except for `in` and `not_in`). */
interface Condition {
  field: string;
  op: FilterOperator;
  values: string[];
}

/** `filter[field]` or `filter[field][op]` as the field and the operator (if there is one); `undefined` when the key is malformed. */
function splitFilterKey(key: string): { field: string; op: string | undefined } | undefined {
  const rest = key.slice("filter[".length);
  const close = rest.indexOf("]");
  if (close < 0) return undefined;
  const field = rest.slice(0, close);
  const after = rest.slice(close + 1);
  if (after === "") return { field, op: undefined };
  if (!after.startsWith("[") || !after.endsWith("]")) return undefined;
  const op = after.slice(1, -1);
  if (op.includes("[") || op.includes("]")) return undefined;
  return { field, op };
}

/** What a filter condition holds as data: a list, a boolean for `null`, or the text. */
function conditionValue(condition: Condition): FilterValue {
  if (condition.op === "in" || condition.op === "not_in") return condition.values;
  if (condition.op === "null") return condition.values[0] === "true";
  return condition.values[0] ?? "";
}

/** The conditions as the `filter` record: a field with only an equal condition is its value, any other is its operators. */
function filterOf(conditions: readonly Condition[]): Record<string, FilterCondition> | undefined {
  if (conditions.length === 0) return undefined;
  const byField = new Map<string, Condition[]>();
  for (const condition of conditions) {
    const list = byField.get(condition.field);
    if (list) list.push(condition);
    else byField.set(condition.field, [condition]);
  }
  const filter: Record<string, FilterCondition> = {};
  for (const [field, list] of byField) {
    const only = list.length === 1 ? list[0] : undefined;
    if (only !== undefined && only.op === "eq") {
      define(filter, field, conditionValue(only));
      continue;
    }
    const ops: OperatorMap = {};
    for (const condition of list) ops[condition.op] = conditionValue(condition);
    define(filter, field, ops);
  }
  return filter;
}

/** The reader behind {@link checkListQuery}: the Rust `Reader`, rule for rule. */
class Reader {
  readonly problems: ListQueryProblem[] = [];
  private readonly conditions: Condition[] = [];
  private readonly sorts: { field: string; descending: boolean }[] = [];
  private sortFull = false;
  private searchText: string | undefined;
  private page: number | undefined;
  private perPage: number | undefined;
  private limit: number | undefined;
  private cursor: string | undefined;
  private before: string | undefined;

  private problem(param: string, message: string): void {
    this.problems.push({ param, message });
  }

  read(input: string): void {
    let text = input;
    if (exceeds(text, MAX_QUERY_BYTES)) {
      this.problem("query", `the query string must not exceed ${MAX_QUERY_BYTES} bytes`);
      text = new TextDecoder("utf-8", { ignoreBOM: true }).decode(encoder.encode(text).subarray(0, MAX_QUERY_BYTES));
    }
    let read = 0;
    // URLSearchParams drops one leading "?", so it gets one of its own to drop.
    for (const [name, value] of new URLSearchParams(`?${text}`)) {
      if (read >= MAX_PARAMS) {
        this.problem("query", `a list query takes at most ${MAX_PARAMS} parameters`);
        break;
      }
      read += 1;
      switch (name) {
        case "sort":
          this.readSort(value);
          break;
        case "search":
          this.readSearch(value);
          break;
        case "page":
          this.readNumber(name, value, MAX_PAGE);
          break;
        case "per_page":
        case "limit":
          this.readNumber(name, value, MAX_PAGE_SIZE);
          break;
        case "cursor":
        case "before":
          this.readCursor(name, value);
          break;
        case "filter":
          this.problem(name, FILTER_HINT);
          break;
        default:
          if (name.startsWith("filter[")) this.readFilter(name, value);
      }
    }
    const numbered = this.page !== undefined || this.perPage !== undefined;
    const byCursor = this.cursor !== undefined || this.before !== undefined || this.limit !== undefined;
    if (numbered && byCursor) {
      const param = this.cursor !== undefined ? "cursor" : this.before !== undefined ? "before" : "limit";
      this.problem(param, "use page and per_page, or cursor and limit, not both");
    }
  }

  private readSort(value: string): void {
    for (const raw of value.split(",")) {
      const term = trimWhite(raw);
      if (term === "") continue;
      const descending = term.startsWith("-");
      const field = descending ? term.slice(1) : term;
      if (!NAME.test(field)) {
        this.problem("sort", `\`${term}\` is not a field name`);
      } else if (this.sorts.some((key) => key.field === field)) {
        this.problem("sort", `\`${field}\` is sorted twice`);
      } else if (this.sorts.length >= MAX_SORTS) {
        if (!this.sortFull) {
          this.sortFull = true;
          this.problem("sort", `sort by at most ${MAX_SORTS} fields`);
        }
      } else {
        this.sorts.push({ field, descending });
      }
    }
  }

  private readSearch(value: string): void {
    const text = trimWhite(value);
    if (text === "") return;
    if (text.includes("\0")) {
      this.problem("search", "must not contain a NUL character");
      return;
    }
    if (exceeds(text, MAX_TEXT)) this.problem("search", `must not exceed ${MAX_TEXT} bytes`);
    this.searchText = clipBytes(text, MAX_TEXT);
  }

  private readNumber(name: "page" | "per_page" | "limit", value: string, max: number): void {
    if (value === "") return;
    let why: string | undefined;
    let n = 0;
    if (!/^[0-9]+$/.test(value)) {
      why = "must be a positive whole number";
    } else {
      // Eight digits hold the largest number allowed; more digits are too large whatever they are.
      const digits = value.replace(/^0+/, "");
      if (digits === "") why = "must be at least 1";
      else if (digits.length > 8 || Number(digits) > max) why = "is too large";
      else n = Number(digits);
    }
    if (why !== undefined) {
      this.problem(name, `${name} ${why}`);
    } else if (name === "page") {
      this.page = n;
    } else if (name === "per_page") {
      this.perPage = n;
    } else {
      this.limit = n;
    }
  }

  private readCursor(name: "cursor" | "before", value: string): void {
    if (value === "") return;
    if (value.includes("\0")) {
      this.problem(name, "must not contain a NUL character");
    } else if (exceeds(value, MAX_CURSOR)) {
      this.problem(name, `must not exceed ${MAX_CURSOR} bytes`);
    } else if (name === "cursor") {
      this.cursor = value;
    } else {
      this.before = value;
    }
  }

  private readFilter(key: string, value: string): void {
    const parts = splitFilterKey(key);
    if (parts === undefined) {
      this.problem(key, FILTER_HINT);
      return;
    }
    const { field, op: opName } = parts;
    if (!NAME.test(field)) {
      this.problem(key, `\`${field}\` is not a field name (letters, digits, _ and . only)`);
      return;
    }
    let op: FilterOperator;
    if (opName === undefined) {
      op = "eq";
    } else if (isOperator(opName)) {
      op = opName;
    } else {
      this.problem(key, `\`${opName}\` is not an operator; use one of: ${OPERATOR_NAMES.join(", ")}`);
      return;
    }
    if (trimWhite(value) === "") return;
    if (value.includes("\0")) {
      this.problem(key, "must not contain a NUL character");
      return;
    }
    const param = paramOf(field, op);
    const takesList = op === "in" || op === "not_in";
    let values: string[];
    if (takesList) {
      values = value.split(",").filter((part) => part !== "");
    } else if (op === "null") {
      if (value === "true" || value === "1") values = ["true"];
      else if (value === "false" || value === "0") values = ["false"];
      else {
        this.problem(param, "must be true or false");
        return;
      }
    } else {
      values = [value];
    }
    if (values.some((part) => exceeds(part, MAX_TEXT))) {
      this.problem(param, `a value must not exceed ${MAX_TEXT} bytes`);
      return;
    }
    if (values.length === 0) return;
    const at = this.conditions.find((condition) => condition.field === field && condition.op === op);
    if (at !== undefined && takesList) {
      for (const part of values) {
        if (at.values.length >= MAX_VALUES) {
          this.problem(param, `takes at most ${MAX_VALUES} values`);
          return;
        }
        at.values.push(part);
      }
    } else if (at !== undefined) {
      this.problem(param, "is given twice; the first is used");
    } else if (this.conditions.length >= MAX_CONDITIONS) {
      this.problem("filter", `a list query takes at most ${MAX_CONDITIONS} filters`);
    } else {
      if (values.length > MAX_VALUES) {
        values = values.slice(0, MAX_VALUES);
        this.problem(param, `takes at most ${MAX_VALUES} values`);
      }
      this.conditions.push({ field, op, values });
    }
  }

  /** What was read, as a list query. */
  query(): ListQuery {
    const query: ListQuery = {};
    const filter = filterOf(this.conditions);
    if (filter !== undefined) query.filter = filter;
    if (this.sorts.length > 0) query.sort = this.sorts.map((key) => (key.descending ? `-${key.field}` : key.field));
    if (this.searchText !== undefined) query.search = this.searchText;
    if (this.page !== undefined) query.page = this.page;
    if (this.perPage !== undefined) query.per_page = this.perPage;
    if (this.cursor !== undefined) query.cursor = this.cursor;
    if (this.before !== undefined) query.before = this.before;
    if (this.limit !== undefined) query.limit = this.limit;
    return query;
  }
}

type ParamSource = URLSearchParams | string | Record<string, string | readonly string[] | undefined>;

/** The query string behind any of the inputs, without its `?`. */
function queryText(input: ParamSource): string {
  if (typeof input === "string") return input.startsWith("?") ? input.slice(1) : input;
  if (input instanceof URLSearchParams) return input.toString();
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(input)) {
    if (value === undefined) continue;
    if (typeof value === "string") params.append(key, value);
    else for (const item of value) params.append(key, item);
  }
  return params.toString();
}

/**
 * Reads a list query from URL parameters: a `URLSearchParams`, a query string
 * (with or without `?`), or a Next.js `searchParams` object. It keeps
 * everything that is right and reports the rest, the way a screen should read
 * an old or edited address: the page still shows. Parameters that are not the
 * list query's are ignored.
 *
 * The rules are the Rust reader's: names are `[A-Za-z_][A-Za-z0-9_.]{0,63}`,
 * operator names are exact and lower case, values that are empty or only white
 * space are left out, `in` and `not_in` add up across repeated keys, any other
 * operator keeps its first value, and the limits (64 conditions, 200 values per
 * `in`, 256 bytes per value or search, 1024 per cursor, 4 sort fields, page
 * 10,000,000, page size 10,000, 1000 parameters, 16 KiB) are enforced.
 */
export function checkListQuery(input: ParamSource): ListQueryCheck {
  const reader = new Reader();
  reader.read(queryText(input));
  return { query: reader.query(), problems: reader.problems };
}

/** The list query in URL parameters: {@link checkListQuery} without the problems. */
export function parseListQuery(input: ParamSource): ListQuery {
  return checkListQuery(input).query;
}

// ---- Writing ---------------------------------------------------------------------------

function isScalar(value: unknown): value is FilterScalar {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean" || value instanceof Date;
}

function isScalarList(value: unknown): value is readonly FilterScalar[] {
  return Array.isArray(value);
}

function isOperatorMap(value: unknown): value is OperatorMap {
  return typeof value === "object" && value !== null && !Array.isArray(value) && !(value instanceof Date);
}

function text(value: FilterScalar): string {
  return value instanceof Date ? value.toISOString() : String(value);
}

/** A value as it is written, or `undefined` when there is nothing to write (blank text, an empty list). */
function written(value: FilterValue | null | undefined): string | undefined {
  if (value === undefined || value === null) return undefined;
  const joined = isScalarList(value) ? value.map(text).filter((part) => part !== "").join(",") : text(value);
  return trimWhite(joined) === "" ? undefined : joined;
}

function appendFilter(params: URLSearchParams, field: string, condition: FilterCondition | undefined): void {
  if (condition === undefined || condition === null) return;
  if (isScalarList(condition)) {
    const value = written(condition);
    if (value !== undefined) params.append(paramOf(field, "in"), value);
  } else if (isOperatorMap(condition)) {
    for (const [op, value] of Object.entries(condition)) {
      const line = written(value);
      if (line !== undefined) params.append(paramOf(field, op as FilterOperator), line);
    }
  } else if (isScalar(condition)) {
    const value = written(condition);
    if (value !== undefined) params.append(paramOf(field, "eq"), value);
  }
}

/**
 * Writes a list query as URL parameters in the canonical order: the filters
 * (grouped by field, an equal filter written plain), then `sort`, `search`,
 * `page`, `per_page`, `cursor`, `before`, `limit`. The encoding is
 * `URLSearchParams`' (a space is `+`, `[` is `%5B`, `,` is `%2C`), and the
 * Rust side writes the same string. Blank values are left out, and the search
 * text is trimmed.
 */
export function serializeListQuery(query: ListQuery): URLSearchParams {
  const params = new URLSearchParams();
  for (const [field, condition] of Object.entries(query.filter ?? {})) appendFilter(params, field, condition);
  const sort = (query.sort ?? []).map((term) => trimWhite(term)).filter((term) => term !== "");
  if (sort.length > 0) params.append("sort", sort.join(","));
  const search = query.search === undefined ? "" : trimWhite(query.search);
  if (search !== "") params.append("search", search);
  if (query.page !== undefined) params.append("page", String(query.page));
  if (query.per_page !== undefined) params.append("per_page", String(query.per_page));
  if (query.cursor) params.append("cursor", query.cursor);
  if (query.before) params.append("before", query.before);
  if (query.limit !== undefined) params.append("limit", String(query.limit));
  return params;
}

// ---- Filters as data -------------------------------------------------------------------

/** A field's condition as operators: a value is `eq`, a list is `in`. */
function operatorsOf(condition: FilterCondition | undefined): OperatorMap {
  if (condition === undefined || condition === null) return {};
  if (isOperatorMap(condition)) return { ...condition };
  if (isScalarList(condition)) return { in: condition };
  return { eq: condition };
}

/** The condition on `field`, if the filter has one of its own (never a name every object has). */
function ownCondition(query: ListQuery, field: string): FilterCondition | undefined {
  const filter = query.filter;
  return filter !== undefined && Object.hasOwn(filter, field) ? filter[field] : undefined;
}

/** `{ eq: value }` alone is the value. */
function tidy(condition: FilterCondition): FilterCondition {
  if (!isOperatorMap(condition)) return condition;
  const keys = Object.keys(condition);
  if (keys.length === 1 && keys[0] === "eq") {
    const eq = condition.eq;
    if (eq !== undefined && !isScalarList(eq)) return eq;
  }
  return condition;
}

/** Whether a condition has anything to write. */
function hasValue(condition: FilterCondition | undefined): boolean {
  if (condition === undefined || condition === null) return false;
  if (isOperatorMap(condition)) return Object.values(condition).some((value) => written(value) !== undefined);
  return written(condition) !== undefined;
}

function withFilter(query: ListQuery, field: string, condition: FilterCondition | undefined): ListQuery {
  // A spread defines each key, so a field called `__proto__` stays a key.
  const filter: Record<string, FilterCondition> = { ...query.filter };
  if (condition === undefined) delete filter[field];
  else define(filter, field, tidy(condition));
  const { filter: _old, ...rest } = query;
  return Object.keys(filter).length > 0 ? { ...rest, filter } : rest;
}

function cloneScalar(value: FilterScalar): FilterScalar {
  return value instanceof Date ? new Date(value.getTime()) : value;
}

function cloneValue(value: FilterValue): FilterValue {
  return isScalarList(value) ? value.map(cloneScalar) : cloneScalar(value);
}

function cloneCondition(condition: FilterCondition): FilterCondition {
  if (condition === undefined || condition === null) return condition;
  if (isOperatorMap(condition)) {
    const ops: OperatorMap = {};
    for (const [op, value] of Object.entries(condition)) {
      if (value !== undefined) define(ops, op, cloneValue(value));
    }
    return ops;
  }
  return cloneValue(condition);
}

/** A copy that shares nothing with `query`, with every key an own key (`structuredClone` is not used: the copy is explicit). */
function cloneQuery(query: ListQuery): ListQuery {
  const out: ListQuery = {};
  if (query.filter !== undefined) {
    const filter: Record<string, FilterCondition> = {};
    for (const [field, condition] of Object.entries(query.filter)) define(filter, field, cloneCondition(condition));
    out.filter = filter;
  }
  if (query.sort !== undefined) out.sort = [...query.sort];
  if (query.search !== undefined) out.search = query.search;
  if (query.page !== undefined) out.page = query.page;
  if (query.per_page !== undefined) out.per_page = query.per_page;
  if (query.cursor !== undefined) out.cursor = query.cursor;
  if (query.before !== undefined) out.before = query.before;
  if (query.limit !== undefined) out.limit = query.limit;
  return out;
}

/** An immutable builder for a {@link ListQuery}: every call returns a new builder. */
export class ListQueryBuilder {
  constructor(private readonly query: ListQuery = {}) {}

  /** `filter[field]=value`. */
  where(field: string, value: FilterValue): ListQueryBuilder;
  /** `filter[field][op]=value`; operators on one field add up. */
  where(field: string, op: FilterOperator, value: FilterValue): ListQueryBuilder;
  where(field: string, opOrValue: FilterValue | FilterOperator, value?: FilterValue): ListQueryBuilder {
    // The builder keeps copies, so a date changed later does not change the query.
    if (value === undefined) {
      const plain = cloneCondition(opOrValue as FilterValue);
      return new ListQueryBuilder(withFilter(this.query, field, isScalarList(plain) ? { in: plain } : plain));
    }
    const ops = operatorsOf(ownCondition(this.query, field));
    define(ops, opOrValue as FilterOperator, cloneValue(value));
    return new ListQueryBuilder(withFilter(this.query, field, ops));
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

  /** Numbered paging: drops `cursor`, `before` and `limit`. */
  page(page: number): ListQueryBuilder {
    const { cursor: _c, before: _b, limit: _l, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, page });
  }

  /** Numbered paging: drops `cursor`, `before` and `limit`. */
  perPage(perPage: number): ListQueryBuilder {
    const { cursor: _c, before: _b, limit: _l, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, per_page: perPage });
  }

  /** Cursor paging: a page's `next_cursor` or `prev_cursor`. Drops `page`, `per_page` and `before`. */
  cursor(cursor: string): ListQueryBuilder {
    const { page: _p, per_page: _pp, before: _b, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, cursor });
  }

  /** Cursor paging, backwards: the page before a position (a list that pages both ways). Drops `page`, `per_page` and `cursor`. */
  before(cursor: string): ListQueryBuilder {
    const { page: _p, per_page: _pp, cursor: _c, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, before: cursor });
  }

  /** Cursor paging: the page size. Drops `page` and `per_page`. */
  limit(limit: number): ListQueryBuilder {
    const { page: _p, per_page: _pp, ...rest } = this.query;
    return new ListQueryBuilder({ ...rest, limit });
  }

  /** The query as data, for the client's `params.query` and query keys. */
  toQuery(): ListQuery {
    return cloneQuery(this.query);
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
  return new ListQueryBuilder(cloneQuery(initial));
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
 * A `querySerializer` for the typed client: the list query's parameters
 * (`filter`, `sort`, `search`, `page`, `per_page`, `cursor`, `before`, `limit`)
 * in list-query format, other objects in brackets (`a[b]=c`), other lists
 * repeated (`ids=a&ids=b`).
 */
export function listQuerySerializer(query: object): string {
  const record = query as Record<string, unknown>;
  const own: ListQuery = {};
  const owned = new Set<string>();
  if (typeof record.filter === "object" && record.filter !== null && !Array.isArray(record.filter)) {
    own.filter = record.filter as ListQuery["filter"];
    owned.add("filter");
  }
  if (typeof record.sort === "string") {
    own.sort = [record.sort];
    owned.add("sort");
  } else if (Array.isArray(record.sort)) {
    own.sort = record.sort as string[];
    owned.add("sort");
  }
  if (typeof record.search === "string") {
    own.search = record.search;
    owned.add("search");
  }
  if (typeof record.page === "number") {
    own.page = record.page;
    owned.add("page");
  }
  if (typeof record.per_page === "number") {
    own.per_page = record.per_page;
    owned.add("per_page");
  }
  if (typeof record.cursor === "string") {
    own.cursor = record.cursor;
    owned.add("cursor");
  }
  if (typeof record.before === "string") {
    own.before = record.before;
    owned.add("before");
  }
  if (typeof record.limit === "number") {
    own.limit = record.limit;
    owned.add("limit");
  }
  const params = serializeListQuery(own);
  for (const [key, value] of Object.entries(record)) {
    if (!owned.has(key)) appendParam(params, key, value);
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
 * then off. Other fields are dropped unless `multi` is set, and a list sorts
 * by at most four fields: a fifth is not added.
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
  if (!placed && next && out.length < MAX_SORTS) out.push(next);
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

/** `query` back on its first page: `page`, `cursor` and `before` dropped (the page size stays). */
export function firstPage(query: ListQuery): ListQuery {
  const { page: _page, cursor: _cursor, before: _before, ...rest } = query;
  return rest;
}

/** `query` with `field`'s filter set (or removed with `undefined`), back on the first page. */
export function setFilter(query: ListQuery, field: string, condition: FilterCondition | undefined): ListQuery {
  const blank = condition === undefined || !hasValue(condition);
  return firstPage(withFilter(query, field, blank ? undefined : condition));
}

/**
 * `query` on numbered page `page` (page 1 is left out of the URL). Numbered
 * paging replaces cursor paging, so `cursor`, `before` and `limit` are dropped.
 */
export function withPage(query: ListQuery, page: number): ListQuery {
  const { page: _page, cursor: _cursor, before: _before, limit: _limit, ...rest } = query;
  return page > 1 ? { ...rest, page } : rest;
}

/**
 * `query` over `defaults`: what `query` says wins. The paging of the two never
 * mixes: a query that pages by cursor takes no default `page` or `per_page`,
 * and a numbered one takes no default `cursor`, `before` or `limit`.
 */
export function withDefaults(query: ListQuery, defaults: ListQuery): ListQuery {
  const base = cloneQuery(defaults);
  const numbered = query.page !== undefined || query.per_page !== undefined;
  const byCursor = query.cursor !== undefined || query.before !== undefined || query.limit !== undefined;
  if (byCursor && !numbered) {
    delete base.page;
    delete base.per_page;
  }
  if (numbered && !byCursor) {
    delete base.cursor;
    delete base.before;
    delete base.limit;
  }
  return { ...base, ...query };
}
