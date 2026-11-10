"use client";

/**
 * A data table on TanStack Table driven by a list query: sorting and paging
 * happen on the server, so the table renders one page and turns header
 * clicks and page buttons into a new list query. Styled with the theme's
 * tokens (`@navree/theme`), logical properties for right-to-left layouts.
 * `useDataTable` is the same table without markup, for your own components.
 *
 * The table also searches, filters, selects and exports as Navree UI's
 * `DataTable` does, in the same words and the same list query: a column says
 * which filter it offers in its `meta`, `bulk` adds the checkboxes and the
 * actions on the selection, `exportUrl` adds the Export link.
 */
import {
  activeFilters,
  bulkRequest,
  defaultTableLabels,
  describeSelection,
  emptySelection,
  exportHref,
  filterKey,
  firstPage,
  groupedCount,
  isFiltered,
  isSelected,
  fill,
  pageRange,
  pageSelection,
  reconcileSelection,
  rowRange,
  selectAllMatching,
  setSelected,
  sortDirection,
  toggleSort,
  toSorting,
  withPage,
  type BulkRequest,
  type FilterColumn,
  type FilterKind,
  type FilterOption,
  type ListQuery,
  type PageMeta,
  type TableLabels,
  type TableSelection,
  type ValueKind,
} from "@navree/client";
import {
  columnVisibilityFeature,
  FlexRender,
  rowPaginationFeature,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type ColumnDef,
  type Header,
  type ReactTable,
  type RowData,
} from "@tanstack/react-table";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { BulkBar, buttonClass, Chips, ConfirmDialog, FilterField, SearchBox, type BulkActionView } from "./data-table-parts.js";

/**
 * The TanStack Table features the table registers: sorting and paging state
 * (both done on the server) and column visibility. Rows come from the core row
 * model as the server sent them.
 */
const features = tableFeatures({ columnVisibilityFeature, rowSortingFeature, rowPaginationFeature });

/** The table's TanStack Table feature set, for TanStack types (`createColumnHelper<NavreeTableFeatures, Deal>()`). */
export type NavreeTableFeatures = typeof features;

/** A column of the table: TanStack Table's `ColumnDef` for the table's features. */
export type NavreeColumnDef<T extends RowData, V = any> = ColumnDef<NavreeTableFeatures, T, V>;

/** What a column's `meta` can say. */
export interface NavreeColumnMeta {
  /** The list-query sort field; the column is sortable when it is set. */
  sortField?: string;
  /** Classes for the column's header and cells (`hidden md:table-cell`). */
  className?: string;
  /** Text alignment: `end` for amounts and counts. */
  align?: "start" | "center" | "end";
  /** The column's name in filter chips and filter controls; its header when that is text. */
  label?: string;
  /** Which filter the column offers: `text`, `range`, `select`, `multi`, `boolean` or `relation`. */
  filter?: FilterKind;
  /** The list-query field the filter names; the sort field, or the column's id, when it is not set. */
  filterField?: string;
  /** How the column's values bind (`int`, `money`, `date`, ...): the type of a range's inputs and the values a filter accepts. */
  kind?: ValueKind;
  /** The column can hold NULL: its filter offers "Empty". */
  nullable?: boolean;
  /** The choices of a `select`, `multi` or `relation` filter, when the page does not pass them in `filterOptions`. */
  options?: FilterOption[];
}

export interface DataTableLabels extends TableLabels {
  previous: string;
  next: string;
  empty: string;
  loading: string;
  pagination: string;
  /** `26–50 of 60`; format numbers for the locale here. */
  range: (from: number, to: number, total: number) => string;
}

export const defaultLabels: DataTableLabels = {
  ...defaultTableLabels,
  previous: "Previous",
  next: "Next",
  empty: "No results.",
  loading: "Loading…",
  pagination: "Pagination",
  range: (from, to, total) => `${from}–${to} of ${total}`,
};

export interface UseDataTableOptions<T extends RowData> {
  columns: NavreeColumnDef<T>[];
  data: T[] | undefined;
  /** The current list query (its `sort` and `page` drive the table), in the one list-query grammar. */
  query: ListQuery;
  /** Called with the next list query: a sort or page change. */
  onQueryChange: (next: ListQuery) => void;
  /** `meta` of the current page, for the page count. */
  meta?: PageMeta;
  getRowId?: (row: T, index: number) => string;
}

export type NavreeTable<T extends RowData> = ReactTable<NavreeTableFeatures, T> & {
  /** Sorts by a column's `meta.sortField` (ascending, descending, off; shift for several). */
  toggleColumnSort: (header: Header<NavreeTableFeatures, T, unknown>, multi?: boolean) => void;
  /** The column's sort direction, if the list is sorted by it. */
  columnSort: (header: Header<NavreeTableFeatures, T, unknown>) => "asc" | "desc" | undefined;
  goToPage: (page: number) => void;
};

function columnMeta<T extends RowData>(header: Header<NavreeTableFeatures, T, unknown>): NavreeColumnMeta {
  return (header.column.columnDef.meta ?? {}) as NavreeColumnMeta;
}

/** The table without markup: TanStack Table with server-side sorting and paging from the list query. */
export function useDataTable<T extends RowData>({ columns, data, query, onQueryChange, meta, getRowId }: UseDataTableOptions<T>): NavreeTable<T> {
  const table = useTable<NavreeTableFeatures, T>({
    features,
    columns,
    data: data ?? [],
    manualSorting: true,
    manualPagination: true,
    enableSorting: false,
    pageCount: meta?.last_page ?? -1,
    state: {
      sorting: toSorting(query.sort),
      pagination: { pageIndex: (meta?.current_page ?? query.page ?? 1) - 1, pageSize: meta?.per_page ?? query.per_page ?? query.limit ?? 15 },
    },
    ...(getRowId ? { getRowId } : {}),
  });

  const toggleColumnSort = (header: Header<NavreeTableFeatures, T, unknown>, multi = false) => {
    const field = columnMeta(header).sortField;
    if (!field) return;
    const { sort: _sort, ...rest } = firstPage(query);
    const sort = toggleSort(query.sort, field, { multi });
    onQueryChange(sort.length > 0 ? { ...rest, sort } : rest);
  };

  return Object.assign(table, {
    toggleColumnSort,
    columnSort: (header: Header<NavreeTableFeatures, T, unknown>) => {
      const field = columnMeta(header).sortField;
      return field ? sortDirection(query.sort, field) : undefined;
    },
    goToPage: (page: number) => onQueryChange(withPage(query, page)),
  });
}

/** An action on the selected rows: `BulkAction` of `@navree/client`, with an icon. */
export type { BulkActionView as DataTableBulkAction };

/** The actions on the selected rows. */
export interface DataTableBulk {
  actions: BulkActionView[];
  /**
   * Called with what to act on: the keys, or every result of the search and filters except some. Answer `false`,
   * or throw, to keep the selection; the selection ends after any other answer. `bulkParams(request)` writes the
   * request as the form post a Navree UI bulk route reads.
   */
  onAction: (request: BulkRequest) => void | boolean | Promise<void | boolean>;
  /** Offer "Select all N results" once the page is selected (default: yes; it needs the list to be counted). */
  selectAllMatching?: boolean;
  /** What the rows' version was when they were drawn, posted with an action on all results so the server can refuse a stale one. */
  version?: string;
}

export interface DataTableProps<T extends RowData> extends UseDataTableOptions<T> {
  /** Shows placeholder rows instead of the data. */
  loading?: boolean;
  /** Makes rows clickable (open a record). */
  onRowClick?: (row: T) => void;
  /** Shown when the page has no rows. */
  empty?: ReactNode;
  labels?: Partial<DataTableLabels>;
  className?: string;
  /** Placeholder rows while loading (default 5). */
  loadingRows?: number;
  /** A name for the table, read by a screen reader. */
  caption?: string;
  /** Adds the search box over the table: the list query's `search`. */
  search?: boolean;
  /** How long typing in the search box or a filter waits for a pause, in milliseconds (default 100). */
  searchDelay?: number;
  /** Where the filters are: in a bar over the table behind a Filters button (`bar`, the default), or in a row under the headings (`row`). */
  filterMode?: "bar" | "row";
  /** The choices of select, multi and relation filters, by filter field. */
  filterOptions?: Record<string, FilterOption[]>;
  /** Adds a checkbox to every row and the actions on the selection. */
  bulk?: DataTableBulk;
  /** A row that cannot be selected. */
  selectableIf?: (row: T) => boolean;
  /** The selection, when you hold it (with `onSelectionChange`); the table holds it itself otherwise. */
  selection?: TableSelection;
  onSelectionChange?: (selection: TableSelection) => void;
  /** The name of a row in its checkbox ("Select Agra renewal"); its first cell's text by default. */
  getRowLabel?: (row: T) => string;
  /** How many results match, for "select all N results"; `meta.total` by default. */
  total?: number;
  /** Adds an Export link to this address, carrying the search, filters and sort (no page). */
  exportUrl?: string;
  /** Your own controls in the toolbar. */
  toolbar?: ReactNode;
}

const ALIGN = { start: "text-start", center: "text-center", end: "text-end" } as const;

function SortMark({ direction }: { direction: "asc" | "desc" | undefined }) {
  return (
    <span aria-hidden="true" className={direction ? "text-fg-2" : "text-fg-3 opacity-0 group-hover:opacity-100"}>
      {direction === "desc" ? "↓" : "↑"}
    </span>
  );
}

/** A column that offers a filter, in the terms of the list query. */
function filterColumnOf(
  column: { id: string; columnDef: { header?: unknown; meta?: unknown } },
  options: Record<string, FilterOption[]> | undefined,
): FilterColumn | undefined {
  const meta = (column.columnDef.meta ?? {}) as NavreeColumnMeta;
  if (!meta.filter) return undefined;
  const field = meta.filterField ?? meta.sortField ?? column.id;
  const header = column.columnDef.header;
  const out: FilterColumn = { field, label: meta.label ?? (typeof header === "string" ? header : column.id), filter: meta.filter };
  if (meta.kind) out.kind = meta.kind;
  if (meta.nullable) out.nullable = true;
  const choices = options?.[field] ?? meta.options;
  if (choices) out.options = choices;
  return out;
}

/** The table, its toolbar, filters, selection, loading and empty states, and the pagination bar. */
export function DataTable<T extends RowData>(props: DataTableProps<T>) {
  const { loading = false, onRowClick, empty, className, loadingRows = 5, meta, query, onQueryChange, bulk, exportUrl, caption, toolbar } = props;
  const labels = { ...defaultLabels, ...props.labels };
  const delay = props.searchDelay ?? 100;
  const table = useDataTable(props);
  const leaf = table.getVisibleLeafColumns();
  const rows = table.getRowModel().rows;
  const selectable = bulk !== undefined;
  const columnCount = leaf.length + (selectable ? 1 : 0);
  const panel = useId();
  const [open, setOpen] = useState(false);
  const filters = table.getAllLeafColumns().flatMap((column) => filterColumnOf(column, props.filterOptions) ?? []);
  const filtered = isFiltered(query);
  const mode = props.filterMode ?? "bar";

  // The selection: the table's own, or yours; "all results" ends with the filters it was made under.
  const [own, setOwn] = useState<TableSelection>(emptySelection);
  const key = filterKey(query);
  const reconciled = reconcileSelection(props.selection ?? own, key);
  const selection = reconciled.selection;
  const [ended, setEnded] = useState(false);
  const change = (next: TableSelection) => {
    setEnded(false);
    setOwn(next);
    props.onSelectionChange?.(next);
  };
  useEffect(() => {
    if (!reconciled.ended) return;
    setOwn(emptySelection());
    props.onSelectionChange?.(emptySelection());
    setEnded(true);
  }, [reconciled.ended]);

  const total = props.total ?? meta?.total;
  const rowIds = rows.map((row) => row.id);
  const selectableIds = rows.filter((row) => props.selectableIf?.(row.original) !== false).map((row) => row.id);
  const status = describeSelection(
    selection,
    { rows: rowIds, selectable: selectableIds, total, canSelectAll: bulk?.selectAllMatching ?? true },
    labels,
    labels.grouping,
  );
  const head = pageSelection(selection, selectableIds);
  const anchor = useRef<string | undefined>(undefined);

  const [asking, setAsking] = useState<BulkActionView | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (action: BulkActionView) => {
    if (!bulk) return;
    setBusy(true);
    try {
      const done = await bulk.onAction(bulkRequest(action.id, selection, { total, version: bulk.version, query }));
      if (done !== false) change(emptySelection());
    } finally {
      setBusy(false);
    }
  };

  const rowLabel = (row: (typeof rows)[number]): string => {
    if (props.getRowLabel) return props.getRowLabel(row.original);
    const cell = row.getVisibleCells()[0];
    const value = cell?.getValue();
    return value === undefined || value === null ? row.id : String(value);
  };

  const bar = filters.length > 0 && mode === "bar";
  const inForce = activeFilters(query, filters, labels).length;
  const hasToolbar = props.search || bar || exportUrl || toolbar;

  return (
    <div data-slot="data-table" className={["surface overflow-hidden", className].filter(Boolean).join(" ")}>
      {hasToolbar ? (
        <div data-slot="table-toolbar" className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
          {props.search ? <SearchBox query={query} onQueryChange={onQueryChange} labels={labels} delay={delay} /> : null}
          {bar ? (
            <button type="button" className={buttonClass} aria-expanded={open} aria-controls={panel} onClick={() => setOpen(!open)}>
              {labels.filters}
              {inForce > 0 ? ` (${inForce})` : ""}
            </button>
          ) : null}
          {toolbar}
          {exportUrl ? (
            <a data-slot="export" className={`${buttonClass} ms-auto inline-flex items-center`} href={exportHref(exportUrl, query)} download>
              {labels.export}
            </a>
          ) : null}
        </div>
      ) : null}
      {bar ? (
        <div id={panel} data-slot="filter-bar" hidden={!open} className="grid gap-3 border-b border-line p-3 sm:grid-cols-2 lg:grid-cols-3">
          {filters.map((column) => (
            <FilterField key={column.field} column={column} query={query} onQueryChange={onQueryChange} labels={labels} delay={delay} />
          ))}
        </div>
      ) : null}
      <Chips query={query} columns={filters} onQueryChange={onQueryChange} labels={labels} />
      {bulk ? (
        <BulkBar
          status={status}
          ended={ended}
          actions={bulk.actions}
          busy={busy}
          labels={labels}
          onAction={(action) => (action.confirm ? setAsking(action) : void run(action))}
          onSelectAll={() => change(selectAllMatching(key))}
          onClear={() => change(emptySelection())}
        />
      ) : null}
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-[13px]" aria-busy={loading || undefined}>
          {caption ? <caption className="sr-only">{caption}</caption> : null}
          <thead>
            {table.getHeaderGroups().map((group, index) => (
              <tr key={group.id} className="border-b border-line">
                {selectable && index === 0 ? (
                  <th scope="col" data-slot="cell-select" className="w-10 px-3">
                    <input
                      type="checkbox"
                      aria-label={labels.selectPage}
                      checked={head.checked}
                      disabled={head.disabled}
                      ref={(box) => {
                        if (box) box.indeterminate = head.indeterminate;
                      }}
                      onChange={() => undefined}
                      onClick={() => change(setSelected(selection, selectableIds, !head.checked))}
                    />
                  </th>
                ) : selectable ? (
                  <th scope="col" className="w-10" />
                ) : null}
                {group.headers.map((header) => {
                  const m = columnMeta(header);
                  const direction = table.columnSort(header);
                  const label = header.isPlaceholder ? null : <FlexRender header={header} />;
                  return (
                    <th
                      key={header.id}
                      scope="col"
                      aria-sort={m.sortField ? (direction === "asc" ? "ascending" : direction === "desc" ? "descending" : "none") : undefined}
                      className={["th-label h-10 px-3 font-medium", ALIGN[m.align ?? "start"], m.className].filter(Boolean).join(" ")}
                    >
                      {m.sortField ? (
                        <button
                          type="button"
                          className="group inline-flex items-center gap-1 rounded-sm hover:text-fg"
                          onClick={(event) => table.toggleColumnSort(header, event.shiftKey)}
                        >
                          {label}
                          <SortMark direction={direction} />
                        </button>
                      ) : (
                        label
                      )}
                    </th>
                  );
                })}
              </tr>
            ))}
            {filters.length > 0 && mode === "row" ? (
              <tr data-slot="filter-row" className="border-b border-line">
                {selectable ? <td /> : null}
                {leaf.map((column) => {
                  const filter = filterColumnOf(column, props.filterOptions);
                  return (
                    <td key={column.id} className="px-3 py-1.5 align-top">
                      {filter ? <FilterField column={filter} query={query} onQueryChange={onQueryChange} labels={labels} delay={delay} inRow /> : null}
                    </td>
                  );
                })}
              </tr>
            ) : null}
          </thead>
          <tbody>
            {loading ? (
              Array.from({ length: loadingRows }, (_, i) => (
                <tr key={`loading-${i}`} className="h-10 border-b border-line last:border-0" data-slot="loading-row">
                  <td colSpan={columnCount} className="px-3">
                    <span className="sr-only">{i === 0 ? labels.loading : ""}</span>
                    <span aria-hidden="true" className="block h-3 w-1/2 rounded-sm bg-ink-2" />
                  </td>
                </tr>
              ))
            ) : rows.length === 0 ? (
              <tr>
                <td colSpan={columnCount} className="px-3 py-10 text-center text-fg-3">
                  {empty ??
                    (filtered ? (
                      <>
                        <span className="block text-fg-2">{labels.emptyFiltered}</span>
                        <span className="block">{labels.emptyFilteredHint}</span>
                      </>
                    ) : (
                      labels.empty
                    ))}
                </td>
              </tr>
            ) : (
              rows.map((row) => {
                const on = selectable && isSelected(selection, row.id);
                const can = selectableIds.includes(row.id);
                return (
                  <tr
                    key={row.id}
                    data-row-id={row.id}
                    data-selected={on ? "" : undefined}
                    className={["h-10 border-b border-line last:border-0", onRowClick ? "cursor-pointer" : "", on ? "bg-ink-2" : "hover:bg-ink-2"].join(" ")}
                    onClick={
                      onRowClick
                        ? (event) => {
                            if ((event.target as Element).closest("[data-slot=cell-select]")) return;
                            onRowClick(row.original);
                          }
                        : undefined
                    }
                  >
                    {selectable ? (
                      <td data-slot="cell-select" className="w-10 px-3">
                        <input
                          type="checkbox"
                          aria-label={fill(labels.selectRow, { label: rowLabel(row) })}
                          value={row.id}
                          checked={on}
                          disabled={!can}
                          onChange={() => undefined}
                          onClick={(event) => {
                            const range = event.shiftKey ? rowRange(selectableIds, anchor.current, row.id) : [row.id];
                            change(setSelected(selection, range, !on));
                            anchor.current = row.id;
                          }}
                        />
                      </td>
                    ) : null}
                    {row.getVisibleCells().map((cell) => {
                      const m = (cell.column.columnDef.meta ?? {}) as NavreeColumnMeta;
                      return (
                        <td key={cell.id} className={["px-3 text-fg-1", ALIGN[m.align ?? "start"], m.className].filter(Boolean).join(" ")}>
                          <FlexRender cell={cell} />
                        </td>
                      );
                    })}
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
      {meta && meta.total > 0 ? <Pagination meta={meta} onPageChange={table.goToPage} labels={labels} /> : null}
      {asking ? (
        <ConfirmDialog
          text={fill(asking.confirm ?? "", { n: groupedCount(status.count, labels.grouping) })}
          labels={labels}
          onCancel={() => setAsking(null)}
          onYes={() => {
            const action = asking;
            setAsking(null);
            void run(action);
          }}
        />
      ) : null}
    </div>
  );
}

export interface PaginationProps {
  meta: PageMeta;
  onPageChange: (page: number) => void;
  labels?: Partial<DataTableLabels>;
}

/** `26–50 of 60` and previous/next buttons for an offset page. */
export function Pagination({ meta, onPageChange, labels: partial }: PaginationProps) {
  const labels = { ...defaultLabels, ...partial };
  const { from, to } = pageRange(meta);
  const button = "h-8 rounded-sm border border-line-2 px-3 text-fg-1 hover:bg-ink-2 disabled:pointer-events-none disabled:opacity-50";
  return (
    <nav aria-label={labels.pagination} className="flex flex-wrap items-center justify-between gap-3 border-t border-line px-3 py-2 text-[13px] text-fg-3">
      <span className="figures">{labels.range(from, to, meta.total)}</span>
      <div className="flex gap-2">
        <button type="button" className={button} disabled={meta.current_page <= 1} onClick={() => onPageChange(meta.current_page - 1)}>
          {labels.previous}
        </button>
        <button
          type="button"
          className={button}
          disabled={meta.current_page >= meta.last_page}
          onClick={() => onPageChange(meta.current_page + 1)}
        >
          {labels.next}
        </button>
      </div>
    </nav>
  );
}
