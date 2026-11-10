/**
 * The words of a table's filters, selection and export, and the two helpers
 * that fill them in. The texts and the `{n}`, `{total}` and `{label}` places
 * are the ones Navree UI's `DataTable` uses (`TableLabels`), so a table says
 * the same thing in every front end. Pass your own for another language.
 */

/** How the digits of counts are grouped: `1,234,567`, or `12,34,567` (lakhs and crores) for `"indian"`. */
export type Grouping = "thousands" | "indian";

/** The words of a table's search, filters, selection and export. */
export interface TableLabels {
  search: string;
  filters: string;
  export: string;
  clearFilters: string;
  /** `{label}` is the filter's text. */
  removeFilter: string;
  emptyFiltered: string;
  emptyFilteredHint: string;
  /** `{label}` is the row's first cell. */
  selectRow: string;
  selectPage: string;
  /** `{n}` is how many rows. */
  selected: string;
  pageSelected: string;
  /** `{n}` is how many results match. */
  selectAll: string;
  allSelected: string;
  /** `{n}` of `{total}`. */
  someSelected: string;
  offPage: string;
  clearSelection: string;
  selectionEnded: string;
  confirmTitle: string;
  confirm: string;
  cancel: string;
  actions: string;
  any: string;
  yes: string;
  no: string;
  min: string;
  max: string;
  from: string;
  to: string;
  findOption: string;
  emptyValue: string;
  /** How the digits of counts are grouped. */
  grouping: Grouping;
}

export const defaultTableLabels: TableLabels = {
  search: "Search",
  filters: "Filters",
  export: "Export",
  clearFilters: "Clear filters",
  removeFilter: "Remove filter {label}",
  emptyFiltered: "No results match these filters.",
  emptyFilteredHint: "Change or clear the search and filters.",
  selectRow: "Select {label}",
  selectPage: "Select all on this page",
  selected: "{n} selected",
  pageSelected: "All {n} on this page are selected.",
  selectAll: "Select all {n} results",
  allSelected: "All {n} results are selected.",
  someSelected: "{n} of {total} results are selected.",
  offPage: "{n} not on this page",
  clearSelection: "Clear selection",
  selectionEnded: "The filter changed, so the selection of all results ended.",
  confirmTitle: "Are you sure?",
  confirm: "Yes, go ahead",
  cancel: "Cancel",
  actions: "Actions",
  any: "Any",
  yes: "Yes",
  no: "No",
  min: "Min",
  max: "Max",
  from: "From",
  to: "To",
  findOption: "Find",
  emptyValue: "Empty",
  grouping: "thousands",
};

/** `template` with each `{name}` replaced by the value of that name (a name with no value stays as written). */
export function fill(template: string, values: Readonly<Record<string, string | number>>): string {
  return template.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const value = Object.hasOwn(values, name) ? values[name] : undefined;
    return value === undefined ? whole : String(value);
  });
}

/** `1234567` as `1,234,567`, or as `12,34,567` for `"indian"`. */
export function groupedCount(value: number, grouping: Grouping = "thousands"): string {
  return value.toLocaleString(grouping === "indian" ? "en-IN" : "en-US");
}
