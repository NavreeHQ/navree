/**
 * A data table on TanStack Vue Table driven by a list query: sorting and
 * paging happen on the server; the table renders one page and emits the next
 * list query. Styled with `@navree/theme` tokens and logical properties, in
 * the same markup as shadcn-vue's Table (`data-slot` attributes), so it sits
 * next to shadcn-vue components. `useDataTable` gives the same table without
 * markup, for your own shadcn-vue `Table` components.
 */
import { pageRange, sortDirection, toggleSort, toSorting, withPage, type ListQuery, type PageMeta } from "@navree/client";
import { FlexRender, getCoreRowModel, useVueTable, type ColumnDef, type Header, type Table } from "@tanstack/vue-table";
import { defineComponent, h, type PropType, type VNode } from "vue";

export interface NavreeColumnMeta {
  /** The list-query sort field; the column is sortable when it is set. */
  sortField?: string;
  /** Classes for the column's header and cells (`hidden md:table-cell`). */
  className?: string;
  align?: "start" | "center" | "end";
}

export interface DataTableLabels {
  previous: string;
  next: string;
  empty: string;
  loading: string;
  pagination: string;
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
  columns: () => ColumnDef<T, any>[];
  data: () => T[] | undefined;
  query: () => ListQuery;
  meta?: () => PageMeta | undefined;
  onQueryChange: (next: ListQuery) => void;
  getRowId?: (row: T, index: number) => string;
}

export interface NavreeTable<T> {
  table: Table<T>;
  toggleColumnSort(header: Header<T, unknown>, multi?: boolean): void;
  columnSort(header: Header<T, unknown>): "asc" | "desc" | undefined;
  goToPage(page: number): void;
}

function columnMeta(header: Header<any, unknown>): NavreeColumnMeta {
  return (header.column.columnDef.meta ?? {}) as NavreeColumnMeta;
}

/** The table without markup: TanStack Vue Table with server-side sorting and paging from the list query. */
export function useDataTable<T>(options: UseDataTableOptions<T>): NavreeTable<T> {
  const table = useVueTable<T>({
    get columns() {
      return options.columns();
    },
    get data() {
      return options.data() ?? [];
    },
    getCoreRowModel: getCoreRowModel(),
    manualSorting: true,
    manualPagination: true,
    enableSorting: false,
    get pageCount() {
      return options.meta?.()?.last_page ?? -1;
    },
    state: {
      get sorting() {
        return toSorting(options.query().sort);
      },
    },
    ...(options.getRowId ? { getRowId: options.getRowId } : {}),
  });

  return {
    table,
    toggleColumnSort(header, multi = false) {
      const field = columnMeta(header).sortField;
      if (!field) return;
      const query = options.query();
      const { page: _page, cursor: _cursor, sort: _sort, ...rest } = query;
      const sort = toggleSort(query.sort, field, { multi });
      options.onQueryChange(sort.length > 0 ? { ...rest, sort } : rest);
    },
    columnSort(header) {
      const field = columnMeta(header).sortField;
      return field ? sortDirection(options.query().sort, field) : undefined;
    },
    goToPage(page) {
      options.onQueryChange(withPage(options.query(), page));
    },
  };
}

const ALIGN = { start: "text-start", center: "text-center", end: "text-end" } as const;
const cls = (...parts: (string | false | undefined)[]) => parts.filter(Boolean).join(" ");

/** `26–50 of 60` and previous/next buttons. */
export const NavreePagination = defineComponent({
  name: "NavreePagination",
  props: {
    meta: { type: Object as PropType<PageMeta>, required: true },
    labels: { type: Object as PropType<Partial<DataTableLabels>>, default: () => ({}) },
  },
  emits: { page: (page: number) => Number.isInteger(page) },
  setup(props, { emit }) {
    return () => {
      const labels = { ...defaultLabels, ...props.labels };
      const { from, to } = pageRange(props.meta);
      const button = "h-8 rounded-sm border border-line-2 px-3 text-fg-1 hover:bg-ink-2 disabled:pointer-events-none disabled:opacity-50";
      return h(
        "nav",
        {
          "aria-label": labels.pagination,
          class: "flex flex-wrap items-center justify-between gap-3 border-t border-line px-3 py-2 text-[13px] text-fg-3",
        },
        [
          h("span", { class: "figures" }, labels.range(from, to, props.meta.total)),
          h("div", { class: "flex gap-2" }, [
            h(
              "button",
              { type: "button", class: button, disabled: props.meta.current_page <= 1, onClick: () => emit("page", props.meta.current_page - 1) },
              labels.previous,
            ),
            h(
              "button",
              {
                type: "button",
                class: button,
                disabled: props.meta.current_page >= props.meta.last_page,
                onClick: () => emit("page", props.meta.current_page + 1),
              },
              labels.next,
            ),
          ]),
        ],
      );
    };
  },
});

/**
 * `<NavreeDataTable :columns :data :meta :query @update:query @row-click>`:
 * the table, its loading and empty states, and the pagination bar.
 */
export const NavreeDataTable = defineComponent({
  name: "NavreeDataTable",
  props: {
    columns: { type: Array as PropType<ColumnDef<any, any>[]>, required: true },
    data: { type: Array as PropType<unknown[]>, default: undefined },
    meta: { type: Object as PropType<PageMeta>, default: undefined },
    query: { type: Object as PropType<ListQuery>, default: () => ({}) },
    loading: { type: Boolean, default: false },
    loadingRows: { type: Number, default: 5 },
    clickable: { type: Boolean, default: false },
    labels: { type: Object as PropType<Partial<DataTableLabels>>, default: () => ({}) },
    getRowId: { type: Function as PropType<(row: any, index: number) => string>, default: undefined },
  },
  emits: {
    "update:query": (query: ListQuery) => typeof query === "object",
    "row-click": (_row: unknown) => true,
  },
  setup(props, { emit, slots }) {
    const navree = useDataTable<any>({
      columns: () => props.columns,
      data: () => props.data,
      query: () => props.query,
      meta: () => props.meta,
      onQueryChange: (next) => emit("update:query", next),
      ...(props.getRowId ? { getRowId: props.getRowId } : {}),
    });

    return () => {
      const labels = { ...defaultLabels, ...props.labels };
      const { table } = navree;
      const columnCount = table.getVisibleLeafColumns().length;
      const rows = table.getRowModel().rows;

      const head = h(
        "thead",
        { "data-slot": "table-header" },
        table.getHeaderGroups().map((group) =>
          h(
            "tr",
            { key: group.id, class: "border-b border-line", "data-slot": "table-row" },
            group.headers.map((header) => {
              const m = columnMeta(header);
              const direction = navree.columnSort(header);
              const label = header.isPlaceholder ? null : h(FlexRender, { render: header.column.columnDef.header, props: header.getContext() });
              return h(
                "th",
                {
                  key: header.id,
                  scope: "col",
                  "data-slot": "table-head",
                  "aria-sort": m.sortField ? (direction === "asc" ? "ascending" : direction === "desc" ? "descending" : "none") : undefined,
                  class: cls("th-label h-10 px-3 font-medium", ALIGN[m.align ?? "start"], m.className),
                },
                m.sortField
                  ? [
                      h(
                        "button",
                        {
                          type: "button",
                          class: "group inline-flex items-center gap-1 rounded-sm hover:text-fg",
                          onClick: (event: MouseEvent) => navree.toggleColumnSort(header, event.shiftKey),
                        },
                        [
                          label,
                          h(
                            "span",
                            { "aria-hidden": "true", class: direction ? "text-fg-2" : "text-fg-3 opacity-0 group-hover:opacity-100" },
                            direction === "desc" ? "↓" : "↑",
                          ),
                        ],
                      ),
                    ]
                  : [label],
              );
            }),
          ),
        ),
      );

      let bodyRows: VNode[];
      if (props.loading) {
        bodyRows = Array.from({ length: props.loadingRows }, (_, i) =>
          h("tr", { key: `loading-${i}`, class: "h-10 border-b border-line last:border-0", "data-slot": "loading-row" }, [
            h("td", { colspan: columnCount, class: "px-3" }, [
              h("span", { class: "sr-only" }, i === 0 ? labels.loading : ""),
              h("span", { "aria-hidden": "true", class: "block h-3 w-1/2 rounded-sm bg-ink-2" }),
            ]),
          ]),
        );
      } else if (rows.length === 0) {
        bodyRows = [
          h("tr", { "data-slot": "table-empty" }, [
            h("td", { colspan: columnCount, class: "px-3 py-10 text-center text-fg-3" }, slots.empty ? slots.empty() : labels.empty),
          ]),
        ];
      } else {
        bodyRows = rows.map((row) =>
          h(
            "tr",
            {
              key: row.id,
              "data-row-id": row.id,
              "data-slot": "table-row",
              class: cls("h-10 border-b border-line last:border-0", props.clickable && "cursor-pointer hover:bg-ink-2"),
              onClick: props.clickable ? () => emit("row-click", row.original) : undefined,
            },
            row.getVisibleCells().map((cell) => {
              const m = (cell.column.columnDef.meta ?? {}) as NavreeColumnMeta;
              return h(
                "td",
                { key: cell.id, "data-slot": "table-cell", class: cls("px-3 text-fg-1", ALIGN[m.align ?? "start"], m.className) },
                [h(FlexRender, { render: cell.column.columnDef.cell, props: cell.getContext() })],
              );
            }),
          ),
        );
      }

      return h("div", { "data-slot": "data-table", class: "surface overflow-hidden" }, [
        h("div", { class: "overflow-x-auto" }, [
          h("table", { "data-slot": "table", class: "w-full border-collapse text-[13px]", "aria-busy": props.loading || undefined }, [
            head,
            h("tbody", { "data-slot": "table-body" }, bodyRows),
          ]),
        ]),
        props.meta && props.meta.total > 0
          ? h(NavreePagination, { meta: props.meta, labels: props.labels, onPage: (page: number) => navree.goToPage(page) })
          : null,
      ]);
    };
  },
});
