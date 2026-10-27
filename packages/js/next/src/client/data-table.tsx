"use client";

/**
 * A data table on TanStack Table driven by a list query: sorting and paging
 * happen on the server, so the table renders one page and turns header
 * clicks and page buttons into a new list query. Styled with the theme's
 * tokens (`@navree/theme`), logical properties for right-to-left layouts.
 * `useDataTable` is the same table without markup, for your own components.
 */
import { pageRange, sortDirection, toggleSort, toSorting, withPage, type ListQuery, type PageMeta } from "@navree/client";
import {
  flexRender,
  getCoreRowModel,
  useReactTable,
  type ColumnDef,
  type Header,
  type Table,
} from "@tanstack/react-table";
import type { ReactNode } from "react";

/** What a column's `meta` can say. */
export interface NavreeColumnMeta {
  /** The list-query sort field; the column is sortable when it is set. */
  sortField?: string;
  /** Classes for the column's header and cells (`hidden md:table-cell`). */
  className?: string;
  /** Text alignment: `end` for amounts and counts. */
  align?: "start" | "center" | "end";
}

export interface DataTableLabels {
  previous: string;
  next: string;
  empty: string;
  loading: string;
  pagination: string;
  /** `26–50 of 60`; format numbers for the locale here. */
  range: (from: number, to: number, total: number) => string;
}

export const defaultLabels: DataTableLabels = {
  previous: "Previous",
  next: "Next",
  empty: "No results.",
  loading: "Loading…",
  pagination: "Pagination",
  range: (from, to, total) => `${from}–${to} of ${total}`,
};

export interface UseDataTableOptions<T> {
  columns: ColumnDef<T, any>[];
  data: T[] | undefined;
  /** The current list query (its `sort` and `page` drive the table). */
  query: ListQuery;
  /** Called with the next list query: a sort or page change. */
  onQueryChange: (next: ListQuery) => void;
  /** `meta` of the current page, for the page count. */
  meta?: PageMeta;
  getRowId?: (row: T, index: number) => string;
}

export type NavreeTable<T> = Table<T> & {
  /** Sorts by a column's `meta.sortField` (ascending, descending, off; shift for several). */
  toggleColumnSort: (header: Header<T, unknown>, multi?: boolean) => void;
  /** The column's sort direction, if the list is sorted by it. */
  columnSort: (header: Header<T, unknown>) => "asc" | "desc" | undefined;
  goToPage: (page: number) => void;
};

function columnMeta<T>(header: Header<T, unknown>): NavreeColumnMeta {
  return (header.column.columnDef.meta ?? {}) as NavreeColumnMeta;
}

/** The table without markup: TanStack Table with server-side sorting and paging from the list query. */
export function useDataTable<T>({ columns, data, query, onQueryChange, meta, getRowId }: UseDataTableOptions<T>): NavreeTable<T> {
  const table = useReactTable<T>({
    columns,
    data: data ?? [],
    getCoreRowModel: getCoreRowModel(),
    manualSorting: true,
    manualPagination: true,
    enableSorting: false,
    pageCount: meta?.last_page ?? -1,
    state: {
      sorting: toSorting(query.sort),
      pagination: { pageIndex: (meta?.current_page ?? query.page ?? 1) - 1, pageSize: meta?.per_page ?? query.per_page ?? 15 },
    },
    ...(getRowId ? { getRowId } : {}),
  });

  const toggleColumnSort = (header: Header<T, unknown>, multi = false) => {
    const field = columnMeta(header).sortField;
    if (!field) return;
    const { page: _page, cursor: _cursor, ...rest } = query;
    const sort = toggleSort(query.sort, field, { multi });
    if (sort.length > 0) onQueryChange({ ...rest, sort });
    else {
      const { sort: _sort, ...unsorted } = rest;
      onQueryChange(unsorted);
    }
  };

  return Object.assign(table, {
    toggleColumnSort,
    columnSort: (header: Header<T, unknown>) => {
      const field = columnMeta(header).sortField;
      return field ? sortDirection(query.sort, field) : undefined;
    },
    goToPage: (page: number) => onQueryChange(withPage(query, page)),
  });
}

export interface DataTableProps<T> extends UseDataTableOptions<T> {
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
}

const ALIGN = { start: "text-start", center: "text-center", end: "text-end" } as const;

function SortMark({ direction }: { direction: "asc" | "desc" | undefined }) {
  return (
    <span aria-hidden="true" className={direction ? "text-fg-2" : "text-fg-3 opacity-0 group-hover:opacity-100"}>
      {direction === "desc" ? "↓" : "↑"}
    </span>
  );
}

/** The table, its loading and empty states, and the pagination bar. */
export function DataTable<T>(props: DataTableProps<T>) {
  const { loading = false, onRowClick, empty, className, loadingRows = 5, meta } = props;
  const labels = { ...defaultLabels, ...props.labels };
  const table = useDataTable(props);
  const columnCount = table.getVisibleLeafColumns().length;
  const rows = table.getRowModel().rows;

  return (
    <div data-slot="data-table" className={["surface overflow-hidden", className].filter(Boolean).join(" ")}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-[13px]" aria-busy={loading || undefined}>
          <thead>
            {table.getHeaderGroups().map((group) => (
              <tr key={group.id} className="border-b border-line">
                {group.headers.map((header) => {
                  const m = columnMeta(header);
                  const direction = table.columnSort(header);
                  const label = header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext());
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
                  {empty ?? labels.empty}
                </td>
              </tr>
            ) : (
              rows.map((row) => (
                <tr
                  key={row.id}
                  data-row-id={row.id}
                  className={["h-10 border-b border-line last:border-0", onRowClick ? "cursor-pointer hover:bg-ink-2" : ""].join(" ")}
                  onClick={onRowClick ? () => onRowClick(row.original) : undefined}
                >
                  {row.getVisibleCells().map((cell) => {
                    const m = (cell.column.columnDef.meta ?? {}) as NavreeColumnMeta;
                    return (
                      <td key={cell.id} className={["px-3 text-fg-1", ALIGN[m.align ?? "start"], m.className].filter(Boolean).join(" ")}>
                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                      </td>
                    );
                  })}
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
      {meta && meta.total > 0 ? <Pagination meta={meta} onPageChange={table.goToPage} labels={labels} /> : null}
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
