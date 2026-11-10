/**
 * Selecting rows of a table, across pages, and what a bulk action posts. The
 * rules are Navree UI's `DataTable`: the browser holds the selection, either
 * the keys chosen or "all the results of this search and filter, except these",
 * and an action posts one of the two with how many rows were seen, so the
 * server can refuse when the set isn't what was shown. "All results" belongs to
 * the search and filters it was chosen under and ends when they change.
 */
import { serializeListQuery, type ListQuery } from "./list-query.js";
import { fill, groupedCount, type Grouping, type TableLabels } from "./table-labels.js";

/** The most keys one post can name, selected or excluded: a server refuses a larger post whole. */
export const MAX_SELECTED_IDS = 10_000;

/** What is selected: these keys (`ids`), or every result except `ex` (`all`, under the filter `key`). */
export interface TableSelection {
  mode: "ids" | "all";
  ids: string[];
  ex: string[];
  key: string;
}

/** Nothing selected. */
export function emptySelection(): TableSelection {
  return { mode: "ids", ids: [], ex: [], key: "" };
}

/** How many rows are selected, given how many results match (`undefined`: not counted). */
export function selectedCount(selection: TableSelection, total: number | undefined): number {
  if (selection.mode === "ids") return selection.ids.length;
  return Math.max(0, (total ?? 0) - selection.ex.length);
}

/** Whether the row with key `id` is selected. */
export function isSelected(selection: TableSelection, id: string): boolean {
  return selection.mode === "all" ? !selection.ex.includes(id) : selection.ids.includes(id);
}

/** The selection with `ids` on or off. In "all" mode the keys held are the ones left out. */
export function setSelected(selection: TableSelection, ids: readonly string[], on: boolean): TableSelection {
  const all = selection.mode === "all";
  const set = new Set(all ? selection.ex : selection.ids);
  for (const id of ids) {
    if (all ? !on : on) set.add(id);
    else set.delete(id);
  }
  return all ? { ...selection, ex: [...set] } : { ...selection, ids: [...set] };
}

/** Every result of the search and filters `key` names. */
export function selectAllMatching(key: string): TableSelection {
  return { mode: "all", ids: [], ex: [], key };
}

/**
 * The selection under the filters `key` names: a selection of all results made under other filters ended,
 * and `ended` says so, for a notice. A selection of keys goes on as it is.
 */
export function reconcileSelection(selection: TableSelection, key: string): { selection: TableSelection; ended: boolean } {
  if (selection.mode === "all" && selection.key !== key) return { selection: emptySelection(), ended: true };
  return { selection, ended: false };
}

/** The keys from `anchor` to `target` in `ids`' order, both included (a shift-click); `[target]` when `anchor` is not in `ids`. */
export function rowRange(ids: readonly string[], anchor: string | undefined, target: string): string[] {
  const a = anchor === undefined ? -1 : ids.indexOf(anchor);
  const b = ids.indexOf(target);
  if (a < 0 || b < 0) return [target];
  return ids.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/** The state of the "select this page" box. */
export interface PageSelection {
  /** Every row that can be selected is. */
  checked: boolean;
  /** Some are, not all. */
  indeterminate: boolean;
  /** No row on the page can be selected. */
  disabled: boolean;
}

/** The "select this page" box, for the rows of the page that can be selected. */
export function pageSelection(selection: TableSelection, selectable: readonly string[]): PageSelection {
  const on = selectable.filter((id) => isSelected(selection, id)).length;
  return { checked: selectable.length > 0 && on >= selectable.length, indeterminate: on > 0 && on < selectable.length, disabled: selectable.length === 0 };
}

/** What a page holds, for the selection's status line. */
export interface SelectionPage {
  /** The key of every row on the page. */
  rows: readonly string[];
  /** The keys of the rows that can be selected. */
  selectable: readonly string[];
  /** How many results match; `undefined` when the list isn't counted. */
  total?: number | undefined;
  /** Whether "select all N results" may be offered (default: yes). */
  canSelectAll?: boolean;
}

/** What the bar over a table says about the selection. */
export interface SelectionStatus {
  /** How many rows are selected. */
  count: number;
  /** The status line: `3 selected · 1 not on this page`, `All 60 results are selected.` */
  text: string;
  /** Whether to offer "Select all N results". */
  offerAll: boolean;
  /** The offer's words. */
  allText: string;
}

/**
 * The status line and the offer to select every result. The offer is made when the whole page is ticked, more
 * results match than the page holds, and nothing else is selected.
 */
export function describeSelection(
  selection: TableSelection,
  page: SelectionPage,
  labels: Pick<TableLabels, "selected" | "pageSelected" | "selectAll" | "allSelected" | "someSelected" | "offPage">,
  grouping: Grouping = "thousands",
): SelectionStatus {
  const total = page.total;
  const count = selectedCount(selection, total);
  const onPage = page.rows.filter((id) => isSelected(selection, id)).length;
  const head = pageSelection(selection, page.selectable);
  const offerAll =
    (page.canSelectAll ?? true) &&
    total !== undefined &&
    selection.mode === "ids" &&
    head.checked &&
    total > page.rows.length &&
    selection.ids.length === onPage;
  const n = groupedCount(count, grouping);
  let text: string;
  if (selection.mode === "all") {
    text =
      selection.ex.length > 0
        ? fill(labels.someSelected, { n, total: groupedCount(total ?? 0, grouping) })
        : fill(labels.allSelected, { n });
  } else if (offerAll) {
    text = fill(labels.pageSelected, { n: groupedCount(onPage, grouping) });
  } else {
    text = fill(labels.selected, { n });
    if (count > onPage) text += ` · ${fill(labels.offPage, { n: groupedCount(count - onPage, grouping) })}`;
  }
  return { count, text, offerAll, allText: fill(labels.selectAll, { n: groupedCount(total ?? 0, grouping) }) };
}

// ---- What a bulk action posts --------------------------------------------------------------------

/** An action on the selected rows. */
export interface BulkAction {
  /** What the server reads. */
  id: string;
  label: string;
  /** Marks it as destroying something. */
  destructive?: boolean;
  /** Asks first. `{n}` in the text is how many rows. */
  confirm?: string;
  /** Shows it disabled, with why. The handler checks again: this is courtesy. */
  denied?: string;
}

/** A bulk action, as the table hands it to your code. */
export interface BulkRequest {
  action: string;
  /** `ids`: these keys; `all`: every result of `query`, except `excluded`. */
  mode: "ids" | "all";
  ids: string[];
  excluded: string[];
  /** How many rows the user saw selected. */
  expected: number;
  /** The version of the rows the user saw, when the list reports one. */
  version?: string;
  /** The search and filters the selection was made under (no page, no sort). */
  query: ListQuery;
}

/**
 * The request for `action` on `selection`. A selection over {@link MAX_SELECTED_IDS} keys is refused
 * (a `RangeError`) rather than cut: cutting it would act on rows the user had left out.
 */
export function bulkRequest(
  action: string,
  selection: TableSelection,
  { total, version, query }: { total?: number | undefined; version?: string | undefined; query: ListQuery },
): BulkRequest {
  const all = selection.mode === "all";
  const keys = all ? selection.ex : selection.ids;
  if (keys.length > MAX_SELECTED_IDS) throw new RangeError(`a bulk action takes at most ${MAX_SELECTED_IDS} keys`);
  if (all && total === undefined) throw new RangeError("all results can be selected only in a counted list");
  const part: ListQuery = {};
  if (query.filter !== undefined) part.filter = query.filter;
  if (query.search !== undefined) part.search = query.search;
  const request: BulkRequest = {
    action,
    mode: selection.mode,
    ids: all ? [] : [...selection.ids],
    excluded: all ? [...selection.ex] : [],
    expected: selectedCount(selection, total),
    query: part,
  };
  if (version) request.version = version;
  return request;
}

/**
 * The request as a form post, with the names Navree UI's `BulkRequest` reads: `action`, `_mode`, `_expected`,
 * `_version`, `ids`, `ex`, and the state as `_q`, so a route that already takes a Navree UI table's bulk post
 * takes this one.
 */
export function bulkParams(request: BulkRequest, { table, path }: { table?: string; path?: string } = {}): URLSearchParams {
  const params = new URLSearchParams();
  params.set("action", request.action);
  if (table) params.set("_table", table);
  if (path) params.set("_path", path);
  params.set("_q", serializeListQuery(request.query).toString());
  if (request.mode === "all") {
    params.set("_mode", "all");
    params.set("_expected", String(request.expected));
    if (request.version) params.set("_version", request.version);
    for (const key of request.excluded) params.append("ex", key);
  } else {
    for (const key of request.ids) params.append("ids", key);
  }
  return params;
}
