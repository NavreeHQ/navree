/**
 * A data table on TanStack Vue Table driven by a list query: sorting and
 * paging happen on the server; the table renders one page and emits the next
 * list query. Styled with `@navree/theme` tokens and logical properties, in
 * the same markup as shadcn-vue's Table (`data-slot` attributes), so it sits
 * next to shadcn-vue components. `useDataTable` gives the same table without
 * markup, for your own shadcn-vue `Table` components.
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
  fill,
  filterKey,
  firstPage,
  groupedCount,
  isFiltered,
  isSelected,
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
  type RowData,
  type VueTable,
} from "@tanstack/vue-table";
import { computed, defineComponent, h, nextTick, ref, shallowRef, watch, type PropType, type VNode, type VNodeChild } from "vue";
import { buttonClass, ConfirmDialog, FilterField, renderBulkBar, renderChips, renderSearchBox, type BulkActionView } from "./data-table-parts.js";

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

export interface NavreeColumnMeta {
  /** The list-query sort field; the column is sortable when it is set. */
  sortField?: string;
  /** Classes for the column's header and cells (`hidden md:table-cell`). */
  className?: string;
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
  columns: () => NavreeColumnDef<T>[];
  data: () => T[] | undefined;
  /** The current list query (its `sort` and `page` drive the table), in the one list-query grammar. */
  query: () => ListQuery;
  meta?: () => PageMeta | undefined;
  onQueryChange: (next: ListQuery) => void;
  getRowId?: (row: T, index: number) => string;
}

export interface NavreeTable<T extends RowData> {
  table: VueTable<NavreeTableFeatures, T>;
  toggleColumnSort(header: Header<NavreeTableFeatures, T, unknown>, multi?: boolean): void;
  columnSort(header: Header<NavreeTableFeatures, T, unknown>): "asc" | "desc" | undefined;
  goToPage(page: number): void;
}

function columnMeta(header: Header<NavreeTableFeatures, any, unknown>): NavreeColumnMeta {
  return (header.column.columnDef.meta ?? {}) as NavreeColumnMeta;
}

/** The table without markup: TanStack Vue Table with server-side sorting and paging from the list query. */
export function useDataTable<T extends RowData>(options: UseDataTableOptions<T>): NavreeTable<T> {
  const table = useTable<NavreeTableFeatures, T>({
    features,
    get columns() {
      return options.columns();
    },
    get data() {
      return options.data() ?? [];
    },
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
      const { sort: _sort, ...rest } = firstPage(query);
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

export type { BulkActionView as DataTableBulkAction };

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

let tables = 0;

/**
 * `<NavreeDataTable :columns :data :meta :query @update:query @row-click>`:
 * the table, its toolbar, filters, selection, loading and empty states, and the pagination bar.
 *
 * - `search`, `filter-mode` (`bar` or `row`), `filter-options`: the search box and the filters of the columns whose `meta` has a `filter`;
 * - `bulk`, `selectable-if`, `v-model:selection`: a checkbox on every row and the actions on the selection;
 * - `export-url`: an Export link carrying the search, filters and sort;
 * - the `toolbar` slot: your own controls.
 */
export const NavreeDataTable = defineComponent({
  name: "NavreeDataTable",
  props: {
    columns: { type: Array as PropType<NavreeColumnDef<any>[]>, required: true },
    data: { type: Array as PropType<unknown[]>, default: undefined },
    meta: { type: Object as PropType<PageMeta>, default: undefined },
    query: { type: Object as PropType<ListQuery>, default: () => ({}) },
    loading: { type: Boolean, default: false },
    loadingRows: { type: Number, default: 5 },
    clickable: { type: Boolean, default: false },
    labels: { type: Object as PropType<Partial<DataTableLabels>>, default: () => ({}) },
    getRowId: { type: Function as PropType<(row: any, index: number) => string>, default: undefined },
    /** A name for the table, read by a screen reader. */
    caption: { type: String, default: undefined },
    /** Adds the search box over the table: the list query's `search`. */
    search: { type: Boolean, default: false },
    /** How long typing in the search box or a filter waits for a pause, in milliseconds. */
    searchDelay: { type: Number, default: 100 },
    /** Where the filters are: in a bar over the table behind a Filters button (`bar`), or in a row under the headings (`row`). */
    filterMode: { type: String as PropType<"bar" | "row">, default: "bar" },
    /** The choices of select, multi and relation filters, by filter field. */
    filterOptions: { type: Object as PropType<Record<string, FilterOption[]>>, default: undefined },
    /** Adds a checkbox to every row and the actions on the selection. */
    bulk: { type: Object as PropType<DataTableBulk>, default: undefined },
    /** A row that cannot be selected. */
    selectableIf: { type: Function as PropType<(row: any) => boolean>, default: undefined },
    /** The selection, when you hold it (`v-model:selection`); the table holds it itself otherwise. */
    selection: { type: Object as PropType<TableSelection>, default: undefined },
    /** The name of a row in its checkbox ("Select Agra renewal"); its first cell's text by default. */
    getRowLabel: { type: Function as PropType<(row: any) => string>, default: undefined },
    /** How many results match, for "select all N results"; `meta.total` by default. */
    total: { type: Number, default: undefined },
    /** Adds an Export link to this address, carrying the search, filters and sort (no page). */
    exportUrl: { type: String, default: undefined },
  },
  emits: {
    "update:query": (query: ListQuery) => typeof query === "object",
    "update:selection": (selection: TableSelection) => typeof selection === "object",
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

    const panel = `navree-filters-${++tables}`;
    const open = ref(false);
    const busy = ref(false);
    const asking = shallowRef<BulkActionView | null>(null);
    const clear = ref<HTMLButtonElement | null>(null);
    let anchor: string | undefined;

    // The selection: the table's own, or yours; "all results" ends with the filters it was made under.
    const own = ref<TableSelection>(emptySelection());
    const ended = ref(false);
    const key = computed(() => filterKey(props.query));
    const reconciled = computed(() => reconcileSelection(props.selection ?? own.value, key.value));
    const change = (next: TableSelection) => {
      ended.value = false;
      own.value = next;
      emit("update:selection", next);
    };
    watch(
      () => reconciled.value.ended,
      (isEnded) => {
        if (!isEnded) return;
        own.value = emptySelection();
        emit("update:selection", emptySelection());
        ended.value = true;
      },
    );

    const total = () => props.total ?? props.meta?.total;
    const run = async (action: BulkActionView) => {
      const bulk = props.bulk;
      if (!bulk) return;
      busy.value = true;
      try {
        const done = await bulk.onAction(bulkRequest(action.id, reconciled.value.selection, { total: total(), version: bulk.version, query: props.query }));
        if (done !== false) change(emptySelection());
      } finally {
        busy.value = false;
      }
    };

    return () => {
      const labels = { ...defaultLabels, ...props.labels };
      const { table } = navree;
      const leaf = table.getVisibleLeafColumns();
      const rows = table.getRowModel().rows;
      const bulk = props.bulk;
      const selectable = bulk !== undefined;
      const columnCount = leaf.length + (selectable ? 1 : 0);
      const filters = table.getAllLeafColumns().flatMap((column) => filterColumnOf(column, props.filterOptions) ?? []);
      const filtered = isFiltered(props.query);
      const bar = filters.length > 0 && props.filterMode === "bar";
      const inForce = activeFilters(props.query, filters, labels).length;
      const onQuery = (next: ListQuery) => emit("update:query", next);

      const selection = reconciled.value.selection;
      const rowIds = rows.map((row) => row.id);
      const selectableIds = rows.filter((row) => props.selectableIf?.(row.original) !== false).map((row) => row.id);
      const status = describeSelection(
        selection,
        { rows: rowIds, selectable: selectableIds, total: total(), canSelectAll: bulk?.selectAllMatching ?? true },
        labels,
        labels.grouping,
      );
      const head = pageSelection(selection, selectableIds);
      const rowLabel = (row: (typeof rows)[number]): string => {
        if (props.getRowLabel) return props.getRowLabel(row.original);
        const value = row.getVisibleCells()[0]?.getValue();
        return value === undefined || value === null ? row.id : String(value);
      };

      const headRows = table.getHeaderGroups().map((group, index) =>
        h("tr", { key: group.id, class: "border-b border-line", "data-slot": "table-row" }, [
          selectable && index === 0
            ? h("th", { scope: "col", "data-slot": "cell-select", class: "w-10 px-3" }, [
                h("input", {
                  type: "checkbox",
                  "aria-label": labels.selectPage,
                  checked: head.checked,
                  indeterminate: head.indeterminate,
                  disabled: head.disabled,
                  onClick: (event: Event) => {
                    change(setSelected(selection, selectableIds, !head.checked));
                    void nextTick(() => {
                      (event.target as HTMLInputElement).checked = pageSelection(reconciled.value.selection, selectableIds).checked;
                    });
                  },
                }),
              ])
            : selectable
              ? h("th", { scope: "col", class: "w-10" })
              : null,
          ...group.headers.map((header) => {
            const m = columnMeta(header);
            const direction = navree.columnSort(header);
            const label = header.isPlaceholder ? null : h(FlexRender, { header });
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
        ]),
      );

      const filterRow =
        filters.length > 0 && props.filterMode === "row"
          ? h("tr", { "data-slot": "filter-row", class: "border-b border-line" }, [
              selectable ? h("td") : null,
              ...leaf.map((column) => {
                const filter = filterColumnOf(column, props.filterOptions);
                return h("td", { key: column.id, class: "px-3 py-1.5 align-top" }, [
                  filter
                    ? h(FilterField, { column: filter, query: props.query, labels, delay: props.searchDelay, inRow: true, "onUpdate:query": onQuery })
                    : null,
                ]);
              }),
            ])
          : null;

      const thead = h("thead", { "data-slot": "table-header" }, [...headRows, filterRow]);

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
            h(
              "td",
              { colspan: columnCount, class: "px-3 py-10 text-center text-fg-3" },
              slots.empty
                ? slots.empty()
                : filtered
                  ? [h("span", { class: "block text-fg-2" }, labels.emptyFiltered), h("span", { class: "block" }, labels.emptyFilteredHint)]
                  : labels.empty,
            ),
          ]),
        ];
      } else {
        bodyRows = rows.map((row) => {
          const on = selectable && isSelected(selection, row.id);
          const can = selectableIds.includes(row.id);
          return h(
            "tr",
            {
              key: row.id,
              "data-row-id": row.id,
              "data-slot": "table-row",
              "data-selected": on ? "" : undefined,
              class: cls("h-10 border-b border-line last:border-0", props.clickable && "cursor-pointer", on ? "bg-ink-2" : "hover:bg-ink-2"),
              onClick: props.clickable
                ? (event: MouseEvent) => {
                    if ((event.target as Element).closest("[data-slot=cell-select]")) return;
                    emit("row-click", row.original);
                  }
                : undefined,
            },
            [
              selectable
                ? h("td", { "data-slot": "cell-select", class: "w-10 px-3" }, [
                    h("input", {
                      type: "checkbox",
                      "aria-label": fill(labels.selectRow, { label: rowLabel(row) }),
                      value: row.id,
                      checked: on,
                      disabled: !can,
                      onClick: (event: MouseEvent) => {
                        const now = reconciled.value.selection;
                        const range = event.shiftKey ? rowRange(selectableIds, anchor, row.id) : [row.id];
                        change(setSelected(now, range, !isSelected(now, row.id)));
                        anchor = row.id;
                        const box = event.target as HTMLInputElement;
                        void nextTick(() => {
                          box.checked = isSelected(reconciled.value.selection, row.id);
                        });
                      },
                    }),
                  ])
                : null,
              ...row.getVisibleCells().map((cell) => {
                const m = (cell.column.columnDef.meta ?? {}) as NavreeColumnMeta;
                return h(
                  "td",
                  { key: cell.id, "data-slot": "table-cell", class: cls("px-3 text-fg-1", ALIGN[m.align ?? "start"], m.className) },
                  [h(FlexRender, { cell })],
                );
              }),
            ],
          );
        });
      }

      const toolbar =
        props.search || bar || props.exportUrl || slots.toolbar
          ? h("div", { "data-slot": "table-toolbar", class: "flex flex-wrap items-center gap-2 border-b border-line px-3 py-2" }, [
              props.search ? renderSearchBox(props.query, labels, props.searchDelay, onQuery) : null,
              bar
                ? h(
                    "button",
                    {
                      type: "button",
                      class: buttonClass,
                      "aria-expanded": String(open.value),
                      "aria-controls": panel,
                      onClick: () => {
                        open.value = !open.value;
                      },
                    },
                    `${labels.filters}${inForce > 0 ? ` (${inForce})` : ""}`,
                  )
                : null,
              slots.toolbar ? slots.toolbar() : null,
              props.exportUrl
                ? h(
                    "a",
                    { "data-slot": "export", class: `${buttonClass} ms-auto inline-flex items-center`, href: exportHref(props.exportUrl, props.query), download: "" },
                    labels.export,
                  )
                : null,
            ])
          : null;

      const filterBar = bar
        ? h(
            "div",
            { id: panel, "data-slot": "filter-bar", hidden: !open.value, class: "grid gap-3 border-b border-line p-3 sm:grid-cols-2 lg:grid-cols-3" },
            filters.map((column) =>
              h(FilterField, { key: column.field, column, query: props.query, labels, delay: props.searchDelay, "onUpdate:query": onQuery }),
            ),
          )
        : null;

      const bulkBar = bulk
        ? renderBulkBar({
            status,
            ended: ended.value,
            actions: bulk.actions,
            busy: busy.value,
            labels,
            clear,
            onAction: (action) => {
              if (action.confirm) asking.value = action;
              else void run(action);
            },
            onSelectAll: () => change(selectAllMatching(key.value)),
            onClear: () => change(emptySelection()),
          })
        : null;

      const dialog = asking.value
        ? h(ConfirmDialog, {
            text: fill(asking.value.confirm ?? "", { n: groupedCount(status.count, labels.grouping) }),
            labels,
            onCancel: () => {
              asking.value = null;
            },
            onYes: () => {
              const action = asking.value;
              asking.value = null;
              if (action) void run(action);
            },
          })
        : null;

      const children: VNodeChild[] = [
        toolbar,
        filterBar,
        renderChips(props.query, filters, labels, onQuery),
        bulkBar,
        h("div", { class: "overflow-x-auto" }, [
          h("table", { "data-slot": "table", class: "w-full border-collapse text-[13px]", "aria-busy": props.loading || undefined }, [
            props.caption ? h("caption", { class: "sr-only" }, props.caption) : null,
            thead,
            h("tbody", { "data-slot": "table-body" }, bodyRows),
          ]),
        ]),
        props.meta && props.meta.total > 0
          ? h(NavreePagination, { meta: props.meta, labels: props.labels, onPage: (page: number) => navree.goToPage(page) })
          : null,
        dialog,
      ];
      return h("div", { "data-slot": "data-table", class: "surface overflow-hidden" }, children);
    };
  },
});
