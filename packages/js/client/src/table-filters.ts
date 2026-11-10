/**
 * A table's search and filters as a list query. Navree UI's `DataTable` offers
 * a column one of seven filters and writes it as `filter[stage]=won`,
 * `filter[stage][in]=won,lost`, `filter[amount][gte]=1&filter[amount][lte]=9`,
 * `filter[title][contains]=sutlej`, `filter[active]=true` or
 * `filter[owner][null]=true`; this file reads and writes the same, so a table
 * in Next.js or Nuxt and one in Navree UI mean the same thing by the same
 * address. A value that doesn't read as its column's kind is left out, as
 * Navree UI leaves it out.
 */
import {
  firstPage,
  serializeListQuery,
  setFilter,
  type FilterCondition,
  type FilterOperator,
  type FilterScalar,
  type FilterValue,
  type ListQuery,
} from "./list-query.js";
import { fill, type TableLabels } from "./table-labels.js";

/** How a column's values bind, so a value in an address reads as what the column holds. */
export type ValueKind = "text" | "int" | "money" | "float" | "decimal" | "date" | "datetime" | "bool" | "uuid";

/** Which filter a column offers. `relation` is a multi filter whose choices are another model's rows. */
export type FilterKind = "text" | "range" | "select" | "multi" | "boolean" | "relation";

/** One choice of a select, multi or relation filter. */
export interface FilterOption {
  value: string;
  label: string;
  /** How many rows hold it, shown beside it. */
  count?: number;
}

/** What a filter holds, as Navree UI's `FilterValue` does. */
export type TableFilterValue =
  | { type: "text"; text: string }
  | { type: "values"; values: string[] }
  | { type: "range"; min?: string; max?: string }
  | { type: "bool"; value: boolean }
  | { type: "empty" };

/** A column that can be filtered. */
export interface FilterColumn {
  /** The list-query field: what `filter[...]` names. */
  field: string;
  /** The column's name in chips and control labels. */
  label: string;
  filter: FilterKind;
  /** How values bind; text unless said. */
  kind?: ValueKind;
  /** Whether the column can hold NULL: adds the "Empty" filter. */
  nullable?: boolean;
  /** The choices of a select, multi or relation filter. */
  options?: readonly FilterOption[];
}

/** A filter in force. */
export interface ActiveFilter {
  field: string;
  label: string;
  /** The chip's text: `Stage: Won, Lost`. */
  text: string;
  value: TableFilterValue;
}

/** The most values one multi filter keeps (the list query's limit for `in`). */
export const MAX_FILTER_VALUES = 200;

const INT = /^[+-]?\d+$/;
const FLOAT = /^[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?$/;
const MONEY = /^-?(?:\d+|\d{1,3}(?:,\d{2,3})*,\d{3})(?:\.\d{1,2})?$|^-?\.\d{1,2}$/;
const DAY = /^(\d{4})-(\d{2})-(\d{2})$/;
const UUID = /^(?:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[0-9a-f]{32})$/i;
const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;

/** Whether `text` reads as `kind`: a number for the numeric kinds, an ISO day for dates. */
export function acceptsValue(kind: ValueKind, text: string): boolean {
  switch (kind) {
    case "text":
      return true;
    case "int": {
      if (!INT.test(text)) return false;
      const n = BigInt(text);
      return n >= I64_MIN && n <= I64_MAX;
    }
    case "money":
      return MONEY.test(text.trim());
    case "float":
    case "decimal":
      return FLOAT.test(text) && Number.isFinite(Number(text));
    case "date":
    case "datetime": {
      const parts = DAY.exec(text);
      if (!parts) return false;
      const [year, month, day] = [Number(parts[1]), Number(parts[2]), Number(parts[3])];
      const at = new Date(Date.UTC(year, month - 1, day));
      return at.getUTCFullYear() === year && at.getUTCMonth() === month - 1 && at.getUTCDate() === day;
    }
    case "bool":
      return text === "true" || text === "false";
    case "uuid":
      return UUID.test(text);
  }
}

/** The type of `<input>` that takes `kind` in a range. */
export function inputType(kind: ValueKind | undefined): "date" | "number" | "text" {
  if (kind === "date" || kind === "datetime") return "date";
  if (kind === "int" || kind === "float" || kind === "decimal") return "number";
  return "text";
}

// ---- Reading a filter from a list query -----------------------------------------------------

type Operators = Partial<Record<FilterOperator, FilterValue>>;

function isList(value: unknown): value is readonly FilterScalar[] {
  return Array.isArray(value);
}

function operatorsOf(condition: FilterCondition | undefined): Operators {
  if (condition === undefined || condition === null) return {};
  if (isList(condition)) return { in: condition };
  if (typeof condition === "object" && !(condition instanceof Date)) return condition;
  return { eq: condition };
}

function scalarText(value: FilterScalar): string {
  return value instanceof Date ? value.toISOString() : String(value);
}

/** The values a condition holds, trimmed, without blanks; text with commas is a list, as the server reads it. */
function valuesOf(value: FilterValue | undefined): string[] {
  if (value === undefined || value === null) return [];
  const parts = isList(value) ? value.map(scalarText) : typeof value === "string" ? value.split(",") : [scalarText(value)];
  return parts.map((part) => part.trim()).filter((part) => part !== "");
}

/** The first value of a condition, as text. */
function firstOf(value: FilterValue | undefined): string | undefined {
  if (value === undefined || value === null || isList(value)) return valuesOf(value)[0];
  const text = scalarText(value).trim();
  return text === "" ? undefined : text;
}

function conditionOn(query: ListQuery, field: string): Operators {
  const filter = query.filter;
  return filter !== undefined && Object.hasOwn(filter, field) ? operatorsOf(filter[field]) : {};
}

/**
 * What `column`'s filter holds in `query`, or `undefined` when it has none. "Is empty" comes first for a
 * column that can hold NULL; a select keeps one value, a multi filter every value it accepts.
 */
export function readFilter(query: ListQuery, column: FilterColumn): TableFilterValue | undefined {
  const ops = conditionOn(query, column.field);
  const kind = column.kind ?? "text";
  if (column.nullable && (ops.null === true || ops.null === "true")) return { type: "empty" };
  switch (column.filter) {
    case "text": {
      const text = firstOf(ops.contains);
      return text === undefined ? undefined : { type: "text", text };
    }
    case "boolean": {
      const text = firstOf(ops.eq);
      return text === "true" || text === "false" ? { type: "bool", value: text === "true" } : undefined;
    }
    case "select": {
      const value = [...valuesOf(ops.eq), ...valuesOf(ops.in)].find((item) => acceptsValue(kind, item));
      return value === undefined ? undefined : { type: "values", values: [value] };
    }
    case "multi":
    case "relation": {
      const values: string[] = [];
      for (const value of [...valuesOf(ops.eq), ...valuesOf(ops.in)]) {
        if (acceptsValue(kind, value) && values.length < MAX_FILTER_VALUES && !values.includes(value)) values.push(value);
      }
      return values.length === 0 ? undefined : { type: "values", values };
    }
    case "range": {
      const min = firstOf(ops.gte);
      const max = firstOf(ops.lte);
      const range: { type: "range"; min?: string; max?: string } = { type: "range" };
      if (min !== undefined && acceptsValue(kind, min)) range.min = min;
      if (max !== undefined && acceptsValue(kind, max)) range.max = max;
      return range.min === undefined && range.max === undefined ? undefined : range;
    }
  }
}

// ---- Writing a filter ------------------------------------------------------------------

/**
 * `query` with `column`'s filter set to `value` (removed with `undefined`, or with a value that holds
 * nothing), back on the first page. Written the way Navree UI writes it: a select as `filter[stage]=won`,
 * a multi filter as `filter[stage][in]=won,lost`, a range as `[gte]` and `[lte]`, text as `[contains]`.
 */
export function writeFilter(query: ListQuery, column: FilterColumn, value: TableFilterValue | undefined): ListQuery {
  const kind = column.kind ?? "text";
  const none = () => setFilter(query, column.field, undefined);
  if (value === undefined) return none();
  switch (value.type) {
    case "empty":
      return column.nullable ? setFilter(query, column.field, { null: true }) : none();
    case "text": {
      const text = value.text.trim();
      return text === "" ? none() : setFilter(query, column.field, { contains: text });
    }
    case "bool":
      return setFilter(query, column.field, { eq: value.value });
    case "values": {
      const values = [...new Set(value.values.map((item) => item.trim()).filter((item) => item !== "" && acceptsValue(kind, item)))].slice(
        0,
        MAX_FILTER_VALUES,
      );
      if (values.length === 0) return none();
      return setFilter(query, column.field, column.filter === "select" ? values[0]! : { in: values });
    }
    case "range": {
      const ops: Operators = {};
      const min = value.min?.trim();
      const max = value.max?.trim();
      if (min && acceptsValue(kind, min)) ops.gte = min;
      if (max && acceptsValue(kind, max)) ops.lte = max;
      return Object.keys(ops).length === 0 ? none() : setFilter(query, column.field, ops);
    }
  }
}

// ---- Search, and the filters as a whole -------------------------------------------------------

/** `query` with its search text set (removed when blank), back on the first page. */
export function withSearch(query: ListQuery, text: string): ListQuery {
  const { search: _search, ...rest } = firstPage(query);
  const trimmed = text.trim();
  return trimmed === "" ? rest : { ...rest, search: trimmed };
}

/** Whether the query searches or filters. */
export function isFiltered(query: ListQuery): boolean {
  if ((query.search ?? "").trim() !== "") return true;
  return Object.keys(query.filter ?? {}).length > 0;
}

/** `query` with no search and no filter, back on the first page; the sort and the page size stay. */
export function clearFilters(query: ListQuery): ListQuery {
  const { filter: _filter, search: _search, ...rest } = firstPage(query);
  return rest;
}

/**
 * The search and filters alone: what a "select all matching" belongs to. The same selection means the
 * same rows only while this stays the same, so the selection of all results ends when it changes.
 */
export function filterKey(query: ListQuery): string {
  const part: ListQuery = {};
  if (query.filter !== undefined) part.filter = query.filter;
  if (query.search !== undefined) part.search = query.search;
  return serializeListQuery(part).toString();
}

// ---- Chips --------------------------------------------------------------------------------

type ChipWords = Pick<TableLabels, "yes" | "no" | "emptyValue">;

function nameOf(column: FilterColumn, value: string): string {
  return column.options?.find((option) => option.value === value)?.label ?? value;
}

/** A filter in force, as its chip says it: `Stage: Won, Lost +2`, `Amount: ≥ 100`, `Active: Yes`. */
export function filterText(column: FilterColumn, value: TableFilterValue, words: ChipWords): string {
  switch (value.type) {
    case "text":
      return `${column.label}: ${value.text}`;
    case "values": {
      const names = value.values.slice(0, 3).map((item) => nameOf(column, item)).join(", ");
      const more = value.values.length > 3 ? ` +${value.values.length - 3}` : "";
      return `${column.label}: ${names}${more}`;
    }
    case "bool":
      return `${column.label}: ${value.value ? words.yes : words.no}`;
    case "empty":
      return `${column.label}: ${words.emptyValue}`;
    case "range":
      if (value.min !== undefined && value.max !== undefined) return `${column.label}: ${value.min} – ${value.max}`;
      if (value.min !== undefined) return `${column.label}: ≥ ${value.min}`;
      if (value.max !== undefined) return `${column.label}: ≤ ${value.max}`;
      return column.label;
  }
}

/** The filters in force, in the order of `columns`. A filter on a field no column names is left out. */
export function activeFilters(query: ListQuery, columns: readonly FilterColumn[], words: ChipWords): ActiveFilter[] {
  const out: ActiveFilter[] = [];
  for (const column of columns) {
    const value = readFilter(query, column);
    if (value) out.push({ field: column.field, label: column.label, text: filterText(column, value, words), value });
  }
  return out;
}

/** The chip's remove button name: `Remove filter Stage: Won`. */
export function removeFilterLabel(labels: Pick<TableLabels, "removeFilter">, text: string): string {
  return fill(labels.removeFilter, { label: text });
}

// ---- What a filter's control looks like -------------------------------------------------------

/** What a framework draws for a filter, so every front end draws the same controls. */
export type FilterControl =
  | { type: "text"; value: string }
  | {
      type: "range";
      min: string;
      max: string;
      inputType: "date" | "number" | "text";
      /** `any` lets a number input take decimals. */
      step?: "any";
      inputMode?: "decimal";
      minLabel: string;
      maxLabel: string;
    }
  | { type: "select"; value: string; choices: FilterOption[]; prompt: string }
  /** Checkboxes; `find` says the list is long enough to want a search box. */
  | { type: "multi"; values: string[]; choices: FilterOption[]; find: boolean }
  /** A multi filter with no list of choices: the key itself. */
  | { type: "keys"; value: string };

/** More choices than this get a "Find" box. */
export const FIND_OPTIONS_OVER = 8;

/** The control for `column`'s filter, with the value `query` holds. */
export function filterControl(query: ListQuery, column: FilterColumn, words: TableLabels): FilterControl {
  const value = readFilter(query, column);
  const choices = [...(column.options ?? [])];
  switch (column.filter) {
    case "text":
      return { type: "text", value: value?.type === "text" ? value.text : "" };
    case "range": {
      const kind = column.kind ?? "text";
      const type = inputType(kind);
      const date = type === "date";
      const control: Extract<FilterControl, { type: "range" }> = {
        type: "range",
        min: value?.type === "range" ? (value.min ?? "") : "",
        max: value?.type === "range" ? (value.max ?? "") : "",
        inputType: type,
        minLabel: date ? words.from : words.min,
        maxLabel: date ? words.to : words.max,
      };
      if (type === "number" && kind !== "int") control.step = "any";
      if (type === "text") control.inputMode = "decimal";
      return control;
    }
    case "boolean":
      return {
        type: "select",
        value: value?.type === "bool" ? String(value.value) : "",
        choices: [
          { value: "true", label: words.yes },
          { value: "false", label: words.no },
        ],
        prompt: words.any,
      };
    case "select":
      return { type: "select", value: value?.type === "values" ? (value.values[0] ?? "") : "", choices, prompt: words.any };
    case "multi":
    case "relation": {
      const values = value?.type === "values" ? value.values : [];
      if (choices.length === 0) return { type: "keys", value: values[0] ?? "" };
      return { type: "multi", values, choices, find: choices.length > FIND_OPTIONS_OVER };
    }
  }
}
